use crate::{Plugin, PluginCallResult, PluginData, PluginResult, PluginError};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use typed_builder::TypedBuilder;
use uuid::Uuid;
use std::result::Result::Err;

pub enum RuntimeResult<P: PluginCallResult> {
    Ok(P::Ok),
    Err(P::Err)
}

impl<P: PluginCallResult> Into<Result<P::Ok, P::Err>> for RuntimeResult<P> {
    fn into(self) -> Result<<P as PluginCallResult>::Ok, <P as PluginCallResult>::Err> {
        match self {
            RuntimeResult::Ok(o) => {
                Ok(o)
            }
            RuntimeResult::Err(e) => {
                Err(e)
            }
        }
    }
}

pub type PluginOpCallId = Uuid;

pub struct PluginOpCall<P: PluginData> {
    pub plugin_data: P,
    pub call_id: PluginOpCallId,
    pub call: P::PluginCall,
}

pub struct PluginOpCallResult<P: PluginCallResult> {
    call_id: PluginOpCallId,
    result: Result<P::Ok, P::Err>,
}

#[derive(TypedBuilder)]
pub struct PluginRuntime<P: PluginData> where P::PluginCall: Send, P::PluginCallResult: PluginCallResult,  {
    #[builder(setter(strip_option))]
    event_loop: Option<Box<dyn Send + Fn(Handle<P>) -> Result<(), String>>>,
    plugin_loader: Box<dyn Fn(P) -> P::PluginCall>,

    #[builder(default=None, setter(skip))]
    result_sender: Option<Sender<PluginOpCallResult<P::PluginCallResult>>>,
    #[builder(default=None, setter(skip))]
    call_sender: Option<Sender<PluginOpCall<P>>>,
    #[builder(default=None, setter(skip))]
    subscribers: Option<Arc<Mutex<HashMap<PluginOpCallId, Sender<RuntimeResult<P::PluginCallResult>>>>>>,
}

impl<P: PluginData> Drop for PluginRuntime<P> {
    fn drop(&mut self) {
        self.call_sender.take();
        self.result_sender.take();
        self.subscribers.take();
    }
}

#[derive(Clone)]
pub struct Handle<P: PluginData> {
    result_sender: Sender<PluginOpCallResult<P::PluginCallResult>>,
    call_receiver: Arc<Mutex<Receiver<PluginOpCall<P>>>>,
}

impl<P: PluginData> Handle<P> {
    pub fn resolve<T: Into<<P::PluginCallResult as PluginCallResult>::Ok>>(&self, id: PluginOpCallId, result: T) {
        if let Err(e) = self.result_sender.send(PluginOpCallResult {
            call_id: id,
            result: Ok(result.into()),
        }) {
            eprintln!("{}", e.to_string())
        }
    }

    pub fn reject<T: Into<<P::PluginCallResult as PluginCallResult>::Err>>(&self, id: PluginOpCallId, result: T) {
        if let Err(e) = self.result_sender.send(PluginOpCallResult {
            call_id: id,
            result: Err(result.into()),
        }) {
            eprintln!("{}", e.to_string());
        }
    }

    pub fn receive(&self) -> Result<PluginOpCall<P>, String> {
        let res = {
            self.call_receiver.lock().unwrap().recv()
        };
        if let Err(ref e) = res {
            return Err(e.to_string());
        }
        Ok(res.unwrap())
    }
}

impl<P: PluginData> PluginRuntime<P> where P::PluginCallResult: 'static + PluginCallResult,  P::PluginCall: 'static + Send {
    pub fn run(&mut self) -> (impl core::future::Future<Output=()>, impl core::future::Future<Output=()>) {
        let (call_sender, call_receiver) = channel();
        let (result_sender, result_receiver) = channel();
        self.call_sender = Some(call_sender);
        let event_loop = self.event_loop.take().unwrap();
        let handle = Handle {
            result_sender: result_sender.clone(),
            call_receiver: Arc::new(Mutex::new(call_receiver)),
        };
        self.result_sender.replace(result_sender);
        self.subscribers.replace(Arc::new(Mutex::new(HashMap::new())));
        let subscribers_cloned = self.subscribers.clone().unwrap();
        (async move {
            loop {
                let res = result_receiver.recv();
                if let Err(e) = res {
                    eprintln!("{}", e.to_string());
                    break;
                }
                let res = res.unwrap();
                let mut subscribers = subscribers_cloned.lock().unwrap();
                if res.call_id.is_nil() {
                    for (_id, sender) in subscribers.drain() {
                        let res = match res.result.clone() {
                            Ok(o) => RuntimeResult::Ok(o),
                            Err(e) => RuntimeResult::Err(e),
                        };
                        if let Err(e) = sender.send(res) {
                            eprintln!("{}", e.to_string());
                            break;
                        }
                    }
                    break;
                }
                let sender = subscribers.remove(&res.call_id).unwrap();
                let res = match res.result {
                    Ok(o) => RuntimeResult::Ok(o),
                    Err(e) => RuntimeResult::Err(e),
                };
                if let Err(e) = sender.send(res) {
                    eprintln!("{}", e.to_string());
                    break;
                }
            }
        },
        async move {
            if let Err(e) = (event_loop)(handle) {
                eprintln!("{}", e);
            }
        })
    }

    pub fn load_plugin(
        &mut self,
        plugin: P,
    ) -> PluginResult<Plugin<P>> {
        if self.call_sender.is_none() {
            return Err(PluginError::FailedToLoad("run runtime first".to_string()))
        }
        if self.subscribers.is_none() {
            return Err(PluginError::FailedToLoad("run runtime first".to_string()))
        }
        let loading_call = (self.plugin_loader)(plugin.clone());
        let pl = Plugin {
            plugin_data: plugin,
            call_sender: self.call_sender.clone().unwrap(),
            subscribers: self.subscribers.clone().unwrap()
        };
        pl.execute(loading_call).and_then(|result| match result {
            Ok(_) => Ok(pl),
            Err(e) => Err(PluginError::FailedToLoad(e.to_string()))
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{build_dummy_runtime};
    use crate::tokio_utils::create_tokio_runtime;

    #[test]
    fn build_runtime() {
        let mut dummy_runtime = build_dummy_runtime();
        let (fut1, fut2) = dummy_runtime.run();
        let runtime = create_tokio_runtime();
        let handle1 = runtime.spawn(fut1);
        let handle2 = runtime.spawn(fut2);
        drop(dummy_runtime);
        let (_res1, _res2) = runtime.block_on(async move {
            tokio::join!(handle1, handle2)
        });
    }
}
