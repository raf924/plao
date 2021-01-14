use crate::{PluginData, PluginResult};

pub trait PluginSource {
    type PluginType: PluginData;
    fn plugins(&self) -> Vec<String>;
    fn open<P: Into<String>>(&mut self, plugin: P) -> PluginResult<Self::PluginType>;
}