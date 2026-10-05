use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::{Connection, Dispatch, QueueHandle};

const LAYER_SHELL_INTERFACE: &str = "zwlr_layer_shell_v1";

struct Probe;

impl Dispatch<WlRegistry, GlobalListContents> for Probe {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

pub fn layer_shell_available() -> bool {
    let Ok(connection) = Connection::connect_to_env() else {
        return false;
    };
    let Ok((globals, _queue)) = registry_queue_init::<Probe>(&connection) else {
        return false;
    };
    globals.contents().with_list(|list| {
        list.iter()
            .any(|global| global.interface == LAYER_SHELL_INTERFACE)
    })
}
