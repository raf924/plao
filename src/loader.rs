use crate::source::PluginSource;
use crate::{PluginData, Plugin};
use crate::runtime::PluginRuntime;

pub struct PluginLoader<Source: PluginSource> {
    source: Source,
    runtime: Option<PluginRuntime<Source::PluginType>>,
}

impl<Source: PluginSource> Drop for PluginLoader<Source> {
    fn drop(&mut self) {
        let runtime = self.runtime.take().unwrap();
        drop(runtime);
    }
}

impl<Source: 'static + PluginSource> PluginLoader<Source> {
    pub fn new(plugin_source: Source, plugin_runtime: PluginRuntime<Source::PluginType>) -> Self {
        PluginLoader {
            source: plugin_source,
            runtime: Some(plugin_runtime),
        }
    }

    pub fn load_plugins(&mut self, excludes: Vec<String>) -> Vec<Plugin<Source::PluginType>> {
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
            runtime.load_plugin(plugin).or_else(|e|{
                eprintln!("failed to load {}: {}", plugin_name, e.to_string());
                Err(e)
            }).ok()
        }).collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::loader::PluginLoader;
    use crate::test_utils::{DummySource, build_dummy_runtime, dummy_event_loop};
    use crate::tokio_utils::create_tokio_runtime;

    #[test]
    fn load_plugin() {
        let mut dummy_runtime = build_dummy_runtime();
        let (fut1, handle) = dummy_runtime.run();
        let runtime = create_tokio_runtime();
        let handle1 = runtime.spawn(fut1);
        let handle2 = runtime.spawn(async move {
            dummy_event_loop(handle)
        });
        let mut dummy_loader = PluginLoader::new(DummySource{}, dummy_runtime);
        let plugins = dummy_loader.load_plugins(vec![]);
        assert_eq!(plugins.len(), 1);
        drop(plugins);
        drop(dummy_loader);
        let (_res1, _res2) = runtime.block_on(async move {
            tokio::join!(handle1, handle2)
        });
    }

    #[test]
    fn exclude_plugin() {
        let mut dummy_runtime = build_dummy_runtime();
        let (fut1, handle) = dummy_runtime.run();
        let runtime = create_tokio_runtime();
        let handle1 = runtime.spawn(fut1);
        let handle2 = runtime.spawn(async move {
            dummy_event_loop(handle)
        });
        let mut dummy_loader = PluginLoader::new(DummySource{}, dummy_runtime);
        let plugins = dummy_loader.load_plugins(vec!["test".to_string()]);
        assert_eq!(plugins.len(), 0);
        drop(dummy_loader);
        let (_res1, _res2) = runtime.block_on(async move {
            tokio::join!(handle1, handle2)
        });
    }
}