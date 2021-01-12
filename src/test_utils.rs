use crate::{PluginCallResult, PluginData, PluginSource, PluginResult};
use std::sync::mpsc::{Receiver, Sender};
use crate::runtime::{PluginOpCallBack, Dispatcher, PluginOpCallResult, PluginRuntime, Handle};
use crate::tokio_utils::create_tokio_runtime;
use std::sync::{Arc, Mutex};

pub(crate) struct DummyPlugin {}

#[derive(Clone)]
pub(crate) struct DummyResult {}

impl PluginCallResult for DummyResult {
    type Ok = String;
    type Err = String;
}

impl PluginData for DummyPlugin {
    type PluginCall = ();
    type PluginCallResult = DummyResult;

    fn name(&self) -> String {
        "test".to_string()
    }
}

pub(crate) struct DummySource {}
impl PluginSource for DummySource {
    type PluginType = DummyPlugin;

    fn plugins(&self) -> Vec<String> {
        vec!["dummy".to_string()]
    }

    fn open<P: Into<String>>(&mut self, plugin: P) -> PluginResult<Self::PluginType> {
        Ok(DummyPlugin {})
    }
}

pub(crate) struct DummyDispatcher {
    pub(crate) receiver: Option<Receiver<Result<String, String>>>,
}

impl Dispatcher for DummyDispatcher {
    type Result = DummyResult;

    fn register(&mut self) -> Result<PluginOpCallBack<Result<String, String>>, String> {
        Ok(PluginOpCallBack::new(0, self.receiver.take().unwrap()))
    }
}

pub(crate) fn build_dummy_runtime<D: 'static + Dispatcher<Result = DummyResult>>(
    sender: Sender<PluginOpCallResult<String, String>>,
    dispatcher: D,
) -> PluginRuntime<(), D::Result> {
    PluginRuntime::builder()
        .result_sender(sender)
        .plugin_loader(Box::new(|_plugin| ()))
        .dispatcher(Arc::new(Mutex::new(dispatcher)))
        .event_loop(Box::new(move |mut handle: Handle<(), D::Result>| {
            let receive = handle.call_receiver();
            let resolve = handle.resolver();
            while let Ok(r) = receive() {
                (resolve)(r.call_id, "hello".to_string());
            }
        }))
        .build()
}