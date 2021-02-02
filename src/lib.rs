mod tokio_utils;

#[cfg(test)]
mod test_utils;
pub mod source;
pub mod loader;
pub mod runtime;

use std::error::Error;
use core::fmt::Formatter;
use core::result::Result;
use std::sync::mpsc::{Sender, channel};
use std::result::Result::Err;
use crate::runtime::{PluginOpCall, RuntimeResult, PluginOpCallId};
use std::sync::{Mutex, Arc};
use std::collections::HashMap;
use uuid::Uuid;

pub type PluginResult<T> = Result<T, PluginError>;

pub trait PluginData: Clone + Send {
    type PluginCall: Send;
    type PluginCallResult: PluginCallResult;
    fn name(&self) -> String;
}

pub trait PluginCallResult: Clone {
    type Ok: Send + Clone;
    type Err: Send + Clone + ToString;
}

pub struct Plugin<P: PluginData> {
    plugin_data: P,
    call_sender: Sender<PluginOpCall<P>>,
    subscribers: Arc<Mutex<HashMap<PluginOpCallId, Sender<RuntimeResult<P::PluginCallResult>>>>>,
}

impl<P: PluginData> Plugin<P> {
    pub fn execute(&self, plugin_call: P::PluginCall) -> PluginResult<Result<<P::PluginCallResult as PluginCallResult>::Ok, <P::PluginCallResult as PluginCallResult>::Err>> {
        let id = Uuid::new_v4();
        let (result_sender, result_receiver) = channel();
        {
            let subscribers = self.subscribers.lock();
            if let Err(ref e) = subscribers {
                return Err(PluginError::RuntimeError(e.to_string()));
            }
            subscribers.unwrap().insert(id, result_sender);
        }
        let res = self.call_sender.send(PluginOpCall {
            plugin_data: self.plugin_data.clone(),
            call_id: id,
            call: plugin_call
        });
        if let Err(ref e) = res {
            return Err(PluginError::RuntimeError(e.to_string()));
        }
        let res = result_receiver.recv();
        if let Err(ref e) = res {
            return Err(PluginError::RuntimeError(e.to_string()));
        }
        Ok(res.unwrap().into())
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
    use crate::tokio_utils::create_tokio_runtime;
    use crate::test_utils::{build_dummy_runtime, DummySource};
    use crate::loader::PluginLoader;

    #[test]
    fn execute() {
        let mut dummy_runtime = build_dummy_runtime();
        let (fut1, fut2) = dummy_runtime.run();
        let runtime = create_tokio_runtime();
        let handle1 = runtime.spawn(fut1);
        let handle2 = runtime.spawn(fut2);
        let mut dummy_loader = PluginLoader::new(DummySource{}, dummy_runtime);
        let plugins = dummy_loader.load_plugins(vec![]);
        let plugin = plugins.first().unwrap();
        let res = plugin.execute(());
        if let Err(e) = res {
            panic!(e)
        }
        let res = res.unwrap();
        assert_eq!(res, Ok("hello".to_string()));
        drop(plugins);
        drop(dummy_loader);
        let (_res1, _res2) = runtime.block_on(async move {
            tokio::join!(handle1, handle2)
        });
    }
}
