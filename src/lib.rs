mod runtime;
mod tokio_utils;

#[cfg(test)]
mod test_utils;

use std::error::Error;
use core::fmt::Formatter;
use core::result::Result;
use std::sync::mpsc::Sender;
use std::result::Result::Err;
use crate::runtime::{Dispatcher, PluginRuntime, PluginOpCall};
use std::sync::{Mutex, Arc};

pub type PluginResult<T> = Result<T, PluginError>;

pub trait PluginData {
    type PluginCall: Send;
    type PluginCallResult: PluginCallResult;
    fn name(&self) -> String;
}

pub trait PluginCallResult {
    type Ok: Send;
    type Err: Send;
}

pub trait PluginSource {
    type PluginType: PluginData;
    fn plugins(&self) -> Vec<String>;
    fn open<P: Into<String>>(&mut self, plugin: P) -> PluginResult<Self::PluginType>;
}

pub struct Plugin<C, R: PluginCallResult> {
    name: String,
    call_sender: Sender<PluginOpCall<C>>,
    dispatcher: Arc<Mutex<dyn Dispatcher<Result=R>>>,
}

impl<C, R: PluginCallResult> Plugin<C, R> {
    fn execute(&mut self, plugin_call: C) -> PluginResult<Result<R::Ok, R::Err>> {
        let callback = {
            self.dispatcher.lock().unwrap().register()
        };
        if let Err(ref e) = callback {
           return Err(PluginError::RuntimeError(e.to_string()));
        }
        let callback = callback.unwrap();
        if let Err(e) = self.call_sender.send(PluginOpCall{
            call_id: callback.op_id(),
            call: plugin_call
        }) {
            return Err(PluginError::RuntimeError(e.to_string()));
        }
        let res = callback.recv();
        if let Err(ref e) = res {
            return Err(PluginError::RuntimeError(e.to_string()));
        }
        let res = res.unwrap();
        Ok(res)
    }
}

pub struct PluginLoader<Source: PluginSource> {
    source: Source,
    runtime: Option<PluginRuntime<<Source::PluginType as PluginData>::PluginCall, <Source::PluginType as PluginData>::PluginCallResult>>,
}

impl<Source: PluginSource> Drop for PluginLoader<Source> {
    fn drop(&mut self) {
        self.runtime.take();
    }
}

impl<Source: 'static + PluginSource> PluginLoader<Source> {
    pub fn new(plugin_source: Source, plugin_runtime: PluginRuntime<<Source::PluginType as PluginData>::PluginCall, <Source::PluginType as PluginData>::PluginCallResult>) -> Self {
        PluginLoader {
            source: plugin_source,
            runtime: Some(plugin_runtime),
        }
    }

    pub fn load_plugins(&mut self, excludes: Vec<String>) -> Vec<Plugin<<Source::PluginType as PluginData>::PluginCall, <Source::PluginType as PluginData>::PluginCallResult>> {
        let source = &mut self.source;
        let runtime = self.runtime.as_mut().unwrap();
        source.plugins().iter().filter_map(|item|{
            if excludes.contains(item) {return None;}
            let plugin = source.open(item);
            if let Err(ref e) = plugin{
                eprintln!("could not load {}: {}", item, e.to_string());
                return None;
            }
            plugin.ok()
        }).filter_map(|plugin|{
            let plugin_name = plugin.name();
            runtime.load_plugin(Box::new(plugin)).or_else(|e|{
                eprintln!("failed to load {}: {}", plugin_name, e.to_string());
                Err(e)
            }).ok()
        }).collect()
    }
}

#[derive(Debug, Clone)]
pub enum PluginError {
    FailedToLoad(String),
    InvalidPlugin(String),
    RuntimeError(String),
}

impl core::fmt::Display for PluginError {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            PluginError::FailedToLoad(e) => {
                writeln!(f, "Failed to load plugin: {}", e)
            }
            PluginError::InvalidPlugin(e) => {
                writeln!(f, "Invalid plugin: {}", e)
            }
            PluginError::RuntimeError(e) => {
                writeln!(f, "Error occured while using plugin: {}", e)
            }
        }
    }
}

impl Error for PluginError {}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{Receiver, channel};
    use crate::runtime::{Dispatcher, PluginOpCallBack, RuntimeDispatcher};
    use crate::{PluginCallResult, Plugin, PluginLoader};
    use std::sync::{Arc, Mutex};
    use std::process::exit;
    use crate::tokio_utils::create_tokio_runtime;
    use crate::test_utils::{DummyDispatcher, DummyPlugin, DummyResult, build_dummy_runtime, DummySource};

    /*#[derive(Clone)]
    struct DummyResult {}

    struct DummyPlugin {}

    impl PluginCallResult for DummyResult {
        type Ok = ();
        type Err = ();
    }

    struct DummyDispatcher {
        receiver: Option<Receiver<Result<(), ()>>>
    }

    impl Dispatcher for DummyDispatcher {
        type Result = DummyResult;

        fn register(&mut self) -> Result<PluginOpCallBack<Result<(), ()>>, String> {
            Ok(PluginOpCallBack::new(0, self.receiver.take().unwrap()))
        }
    }*/

    #[test]
    fn execute() {
        let (sender, receiver) = channel();
        let (call_sender, call_receiver) = channel();
        let dispatcher = DummyDispatcher {
            receiver: Some(receiver),
        };
        let plugin = DummyPlugin {};
        let mut pl: Plugin<(), DummyResult> = Plugin {
            name: "test".to_string(),
            call_sender,
            dispatcher: Arc::new(Mutex::new(dispatcher)),
        };
        let res = sender.send(Ok("".to_string()));
        if let Err(ref e) = res {
            panic!(e.to_string());
        }
        let res = pl.execute(());
        if let Err(ref e) = res {
            panic!(e.to_string());
        }
        let res = res.unwrap();
        assert!(res.is_ok())
    }

    #[test]
    fn loader() {
        std::panic::set_hook(Box::new(|e| {
            eprintln!("{}", e.to_string());
            exit(1);
        }));
        let (result_sender, result_receiver) = channel();
        let mut dispatcher: RuntimeDispatcher<DummyResult> = RuntimeDispatcher::new(result_receiver);
        let dispatcher_future = dispatcher.run();
        let mut plugin_runtime = build_dummy_runtime(result_sender, dispatcher);
        let future = plugin_runtime.run();
        let mut dummy_loader = PluginLoader::new(DummySource {}, plugin_runtime);
        let runtime = create_tokio_runtime();
        let runtime_handle = runtime.spawn(future);
        let dispatcher_handle = runtime.spawn(dispatcher_future);
        let plugins = dummy_loader.load_plugins(vec![]);
        assert_eq!(plugins.len(), 1);
        drop(dummy_loader);
        drop(plugins);
        let (rres, dres) = runtime.block_on(async {
            tokio::join!(runtime_handle, dispatcher_handle)
        });
        if let Err(e) = rres {
            panic!(e);
        }
        if let Err(e) = dres {
            panic!(e);
        }
    }
}
