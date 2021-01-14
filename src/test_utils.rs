use crate::{PluginCallResult, PluginData, PluginResult};
use crate::runtime::{PluginRuntime, Handle};
use crate::source::PluginSource;

#[derive(Clone)]
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
        vec!["test".to_string()]
    }

    fn open<P: Into<String>>(&mut self, _plugin: P) -> PluginResult<Self::PluginType> {
        Ok(DummyPlugin {})
    }
}

pub(crate) fn build_dummy_runtime() -> PluginRuntime<(), DummyResult, DummyPlugin> {
    PluginRuntime::builder()
        .plugin_loader(Box::new(|_plugin| ()))
        .event_loop(Box::new(move |mut handle: Handle<DummyPlugin>| {
            let receive = handle.call_receiver();
            let resolve = handle.resolver();
            while let Ok(r) = receive() {
                (resolve)(r.call_id, "hello".to_string());
            }
        }))
        .build()
}