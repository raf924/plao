use crate::{Plugin, PluginCallResult, PluginData, PluginResult, PluginError};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use typed_builder::TypedBuilder;
use uuid::Uuid;
use std::result::Result::Err;

pub type PluginOpCallId = Uuid;

pub struct PluginOpCall<P: PluginData, T> {
    pub plugin_data: P,
    pub call_id: PluginOpCallId,
    pub call: T,
}

pub struct PluginOpCallResult<Success, Failure> {
    call_id: PluginOpCallId,
    result: Result<Success, Failure>,
}

#[derive(TypedBuilder)]
pub struct PluginRuntime<C: Send, R: PluginCallResult, P: PluginData<PluginCall=C, PluginCallResult=R>> {
    #[builder(setter(strip_option))]
    event_loop: Option<Box<dyn Send + Fn(Handle<P>) -> ()>>,
    plugin_loader: Box<dyn Fn(P) -> C>,

    #[builder(default=None, setter(skip))]
    result_sender: Option<Sender<PluginOpCallResult<R::Ok, R::Err>>>,
    #[builder(default=None, setter(skip))]
    call_sender: Option<Sender<PluginOpCall<P, C>>>,
    #[builder(default=None, setter(skip))]
    subscribers: Option<Arc<Mutex<HashMap<PluginOpCallId, Sender<Result<R::Ok, R::Err>>>>>>,
}

impl<C: Send, R: PluginCallResult, P: PluginData<PluginCall=C, PluginCallResult=R>> Drop for PluginRuntime<C, R, P> {
    fn drop(&mut self) {
        self.call_sender.take();
        self.result_sender.take();
        self.subscribers.take();
    }
}

pub struct Handle<P: PluginData> {
    result_sender: Sender<PluginOpCallResult<<P::PluginCallResult as PluginCallResult>::Ok, <P::PluginCallResult as PluginCallResult>::Err>>,
    call_receiver: Option<Receiver<PluginOpCall<P, P::PluginCall>>>,
}

impl<P: PluginData> Handle<P> {
    pub fn resolver<T: Into<<P::PluginCallResult as PluginCallResult>::Ok>>(&self) -> impl Fn(PluginOpCallId, T) {
        let sender = self.result_sender.clone();
        move |id, result| {
            if let Err(e) = sender.send(PluginOpCallResult {
                call_id: id,
                result: Ok(result.into()),
            }) {
                eprintln!("{}", e.to_string())
            }
        }
    }

    pub fn rejecter<T: Into<<P::PluginCallResult as PluginCallResult>::Err>>(&self) -> impl Fn(PluginOpCallId, T) {
        let sender = self.result_sender.clone();
        move |id, result| {
            if let Err(e) = sender.send(PluginOpCallResult {
                call_id: id,
                result: Err(result.into()),
            }) {
                eprintln!("{}", e.to_string());
            }
        }
    }

    pub fn call_receiver(&mut self) -> impl Fn() -> Result<PluginOpCall<P, P::PluginCall>, String> {
        let receiver = self.call_receiver.take();
        move || {
            if receiver.is_none() {
                return Err("call_receiver already used".to_string());
            }
            let res = receiver.as_ref().unwrap().recv();
            if let Err(ref e) = res {
                return Err(e.to_string());
            }
            Ok(res.unwrap())
        }
    }
}

impl<C: 'static + Send, R: 'static + PluginCallResult, P: PluginData<PluginCall=C, PluginCallResult=R>> PluginRuntime<C, R, P> {
    pub fn run(&mut self) -> (impl core::future::Future<Output=()>, impl core::future::Future<Output=()>) {
        let (call_sender, call_receiver) = channel();
        let (result_sender, result_receiver) = channel();
        self.call_sender = Some(call_sender);
        let event_loop = self.event_loop.take().unwrap();
        let handle = Handle {
            result_sender: result_sender.clone(),
            call_receiver: Some(call_receiver),
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
                let sender: Sender<Result<R::Ok, R::Err>> = subscribers.remove(&res.call_id).unwrap();
                if let Err(e) = sender.send(res.result) {
                    eprintln!("{}", e.to_string());
                    break;
                }
            }
        },
        async move {
            (event_loop)(handle);
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
        pl.execute(loading_call).and_then(|_result| Ok(pl))
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
