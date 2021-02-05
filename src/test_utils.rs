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

pub(crate) fn dummy_event_loop(handle: Handle<DummyPlugin>) -> Result<(), String> {
    while let Ok(r) = handle.receive() {
        handle.resolve(r.call_id, "hello".to_string());
    }
    Ok(())
}

pub(crate) fn build_dummy_runtime() -> PluginRuntime<DummyPlugin> {
    PluginRuntime::builder()
        .plugin_loader(Box::new(|_plugin| ()))
        .build()
}