use crate::{Plugin, PluginCallResult, PluginData, PluginResult};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, RecvError, Sender};
use std::sync::{Arc, Mutex};
use typed_builder::TypedBuilder;

pub type PluginOpCallId = i64;

pub struct PluginOpCall<T> {
    pub call_id: PluginOpCallId,
    pub call: T,
}

pub struct PluginOpCallResult<Success, Failure> {
    call_id: PluginOpCallId,
    result: Result<Success, Failure>,
}

pub struct PluginOpCallBack<T> {
    op_id: PluginOpCallId,
    receiver: Receiver<T>,
}

impl<T> PluginOpCallBack<T> {
    pub fn new(op_id: PluginOpCallId, receiver: Receiver<T>) -> Self {
        PluginOpCallBack { op_id, receiver }
    }
    pub fn op_id(&self) -> PluginOpCallId {
        self.op_id
    }

    pub fn recv(&self) -> Result<T, RecvError> {
        self.receiver.recv()
    }
}

pub trait Dispatcher {
    type Result: PluginCallResult;
    fn register(
        &mut self,
    ) -> Result<
        PluginOpCallBack<
            Result<<Self::Result as PluginCallResult>::Ok, <Self::Result as PluginCallResult>::Err>,
        >,
        String,
    >;
}

#[derive(TypedBuilder)]
pub struct PluginRuntime<C, R: PluginCallResult> {
    #[builder(setter(strip_option))]
    event_loop: Option<Box<dyn Send + Fn(Handle<C, R>) -> ()>>,

    plugin_loader: Box<dyn Fn(Box<dyn PluginData<PluginCall = C, PluginCallResult = R>>) -> C>,

    dispatcher: Arc<Mutex<dyn Dispatcher<Result = R>>>,

    #[builder(setter(strip_option))]
    result_sender: Option<Sender<PluginOpCallResult<R::Ok, R::Err>>>,
    #[builder(default=None, setter(skip))]
    call_sender: Option<Sender<PluginOpCall<C>>>,
}

impl<C, R: PluginCallResult> Drop for PluginRuntime<C, R> {
    fn drop(&mut self) {
        self.call_sender.take();
        self.result_sender.take();
    }
}

pub struct Handle<C, R: PluginCallResult> {
    result_sender: Sender<PluginOpCallResult<R::Ok, R::Err>>,
    call_receiver: Option<Receiver<PluginOpCall<C>>>,
}

impl<C, R: PluginCallResult> Handle<C, R> {
    pub fn resolver<T: Into<R::Ok>>(&self) -> impl Fn(PluginOpCallId, T) {
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

    pub fn rejecter<T: Into<R::Err>>(&self) -> impl Fn(PluginOpCallId, T) {
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

    pub fn call_receiver(&mut self) -> impl Fn() -> Result<PluginOpCall<C>, String> {
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

impl<C: 'static + Send, R: 'static + PluginCallResult> PluginRuntime<C, R> {
    pub fn run(&mut self) -> impl Send + core::future::Future<Output=()> {
        let (call_sender, call_receiver) = channel();
        self.call_sender = Some(call_sender);
        let event_loop = self.event_loop.take().unwrap();
        let handle = Handle {
            result_sender: self.result_sender.clone().unwrap(),
            call_receiver: Some(call_receiver),
        };
        async move {
            (event_loop)(handle);
        }
    }

    pub fn load_plugin(
        &mut self,
        plugin: Box<dyn PluginData<PluginCall = C, PluginCallResult = R>>,
    ) -> PluginResult<Plugin<C, R>> {
        let name = plugin.name();
        let loading_call = (self.plugin_loader)(plugin);
        let mut pl = Plugin {
            name,
            call_sender: self.get_call_sender(),
            dispatcher: self.dispatcher.clone(),
        };
        pl.execute(loading_call).and_then(|_result| Ok(pl))
    }
    pub fn get_call_sender(&self) -> Sender<PluginOpCall<C>> {
        self.call_sender.clone().unwrap()
    }
}

pub struct RuntimeDispatcher<R: PluginCallResult> {
    result_receiver: Option<Receiver<PluginOpCallResult<R::Ok, R::Err>>>,
    results: Arc<Mutex<HashMap<PluginOpCallId, Result<R::Ok, R::Err>>>>,
    subscribers: Arc<Mutex<HashMap<PluginOpCallId, Sender<Result<R::Ok, R::Err>>>>>,
    counter: i64,
}

impl<R: PluginCallResult> Dispatcher for RuntimeDispatcher<R> {
    type Result = R;

    fn register(
        &mut self,
    ) -> Result<
        PluginOpCallBack<
            Result<<Self::Result as PluginCallResult>::Ok, <Self::Result as PluginCallResult>::Err>,
        >,
        String,
    > {
        if self.counter == i64::MAX {
            return Err("too many ops".to_string());
        }
        if self.counter > 0 {
            let subscribers = self.subscribers.lock().unwrap();
            if subscribers.len() == 0 {
                self.counter = 0;
            }
            let mut keys: Vec<&PluginOpCallId> = subscribers.keys().collect();
            keys.sort_unstable();
            keys.reverse();
            self.counter = **keys.first().unwrap();
        }
        self.counter += 1;
        let id = self.counter;
        let mut subscribers = self.subscribers.lock().unwrap();
        let mut results = self.results.lock().unwrap();
        if subscribers.contains_key(&id) {
            return Err("already registered".to_string());
        }
        let (result_sender, result_receiver) = channel();
        if results.contains_key(&id) {
            result_sender.send(results.remove(&id).unwrap());
        } else {
            subscribers.insert(id, result_sender);
        }
        Ok(PluginOpCallBack {
            op_id: id,
            receiver: result_receiver,
        })
    }
}

impl<R: 'static + PluginCallResult> RuntimeDispatcher<R> {
    pub fn run(&mut self) -> impl core::future::Future<Output=()> {
        let result_receiver = Mutex::new(self.result_receiver.take().unwrap());
        let results_cloned = self.results.clone();
        let subscribers_cloned = self.subscribers.clone();
        async move {
            while let Ok(receiver) = result_receiver.lock() {
                let res = receiver.recv();
                if let Err(e) = res {
                    eprintln!("{}", e.to_string());
                    break;
                }
                let res = res.unwrap();
                let mut subscribers = subscribers_cloned.lock().unwrap();
                let id = res.call_id;
                if !subscribers.contains_key(&id) {
                    results_cloned.lock().unwrap().insert(id, res.result);
                    continue;
                }
                let sender: Sender<Result<R::Ok, R::Err>> = subscribers.remove(&id).unwrap();
                if let Err(e) = sender.send(res.result) {
                    eprintln!("{}", e.to_string());
                    break;
                }
            }
        }
    }
    pub fn new(result_receiver: Receiver<PluginOpCallResult<R::Ok, R::Err>>) -> Self {
        let results = Arc::new(Mutex::new(HashMap::new()));
        let subscribers = Arc::new(Mutex::new(HashMap::new()));
        RuntimeDispatcher {
            result_receiver: Some(result_receiver),
            results,
            subscribers,
            counter: 0,
        }
    }
}


#[cfg(test)]
mod tests {
    use std::sync::mpsc::channel;
    use crate::test_utils::{build_dummy_runtime, DummyDispatcher};

    #[test]
    fn build_runtime() {
        let (sender, receiver) = channel();
        let (result_sender, result_receiver) = channel();
        let mut runtime = build_dummy_runtime(
            sender,
            DummyDispatcher {
                receiver: Some(result_receiver),
            },
        );
        runtime.run();
    }
}
