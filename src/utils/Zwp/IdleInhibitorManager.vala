using GLib;

namespace Utils {
    public class IdleInhibitorManager : Object {
        private static IdleInhibitorManager? instance = null;
        
        private bool _active = false;
        public bool active {
            get { return _active; }
            set {
                if (_active != value) {
                    _active = value;
                    update_inhibitor();
                }
            }
        }
        
        private bool setup_successful = false;
        private Wl.Display? display = null;
        private Wl.Registry? registry = null;
        private Wl.Compositor? compositor = null;
        private Wl.Surface? surface = null;
        private Zwp.IdleInhibitManagerV1? idle_inhibit_manager = null;
        private Zwp.IdleInhibitorV1? idle_inhibitor = null;

        private IdleInhibitorManager() {
            setup_wayland();
        }

        public static IdleInhibitorManager get_instance() {
            if (instance == null) {
                instance = new IdleInhibitorManager();
            }
            return instance;
        }

        private void setup_wayland() {
            
            var gdk_display = Gdk.Display.get_default();
            if (gdk_display == null) {
                critical("[IdleInhibitorManager] FAILED: No default GDK display");
                return;
            }
            
            unowned string? wayland_display_name = Environment.get_variable("WAYLAND_DISPLAY");
            if (wayland_display_name == null) {
                critical("[IdleInhibitorManager] FAILED: Not running on Wayland (WAYLAND_DISPLAY not set)");
                return;
            }
            
            display = new Wl.Display.connect(wayland_display_name);
            if (display == null) {
                critical("[IdleInhibitorManager] FAILED: Could not connect to Wayland display '%s'", wayland_display_name);
                return;
            }

            registry = display.get_registry();
            if (registry == null) {
                critical("[IdleInhibitorManager] FAILED: Could not get Wayland registry");
                return;
            }

            Wl.RegistryListener listener = Wl.RegistryListener() {
                global = on_registry_global,
                global_remove = on_registry_global_remove
            };

            registry.add_listener(listener, this);
            display.roundtrip();

            if (compositor == null) {
                critical("[IdleInhibitorManager] FAILED: wl_compositor not available");
                return;
            }

            if (idle_inhibit_manager == null) {
                critical("[IdleInhibitorManager] FAILED: zwp_idle_inhibit_manager_v1 not available");
                critical("[IdleInhibitorManager] Your compositor may not support the idle-inhibit protocol");
                return;
            }

            surface = compositor.create_surface();
            if (surface == null) {
                critical("[IdleInhibitorManager] FAILED: Could not create Wayland surface");
                return;
            }
            
            setup_successful = true;
        }

        private static void on_registry_global(void* data, Wl.Registry registry, uint32 name, string @interface, uint32 version) {
            IdleInhibitorManager self = (IdleInhibitorManager) data;

            if (@interface == "wl_compositor") {
                self.compositor = registry.bind<Wl.Compositor>(name, ref Wl.compositor_interface, version);
            } else if (@interface == "zwp_idle_inhibit_manager_v1") {
                self.idle_inhibit_manager = registry.bind<Zwp.IdleInhibitManagerV1>(name, ref Zwp.idle_inhibit_manager_v1_interface, version);
            }
        }

        private static void on_registry_global_remove(void* data, Wl.Registry registry, uint32 name) {
            // Handle removal if needed
        }

        private void update_inhibitor() {
            if (!setup_successful) {
                warning("[IdleInhibitorManager] Cannot update: Setup failed");
                return;
            }

            if (surface == null || idle_inhibit_manager == null || display == null) {
                warning("[IdleInhibitorManager] Cannot update: Missing Wayland resources");
                return;
            }

            if (_active) {
                if (idle_inhibitor == null) {
                    idle_inhibitor = idle_inhibit_manager.create_inhibitor(surface);
                    if (idle_inhibitor == null) {
                        critical("[IdleInhibitorManager] FAILED to create inhibitor!");
                        _active = false;
                        return;
                    }
                    surface.commit();
                    display.roundtrip();
                }
            } else {
                if (idle_inhibitor != null) {
                    idle_inhibitor = null; 
                    surface.commit();
                    display.roundtrip();
                }
            }
        }

        public bool is_available() {
            return setup_successful;
        }

        ~IdleInhibitorManager() {
            idle_inhibitor = null;
            surface = null;
            idle_inhibit_manager = null;
            compositor = null;
            registry = null;
            display = null;
        }
    }
}
