/* Vala bindings for zwp_idle_inhibit_manager_v1 Wayland protocol */

[CCode (cheader_filename = "idle-inhibit-client-protocol.h")]
namespace Zwp {
    [CCode (cname = "struct zwp_idle_inhibit_manager_v1", free_function = "zwp_idle_inhibit_manager_v1_destroy")]
    [Compact]
    public class IdleInhibitManagerV1 : Wl.Proxy {
        public void set_user_data (void* user_data);
        public void* get_user_data ();
        public uint32 get_version ();
        [CCode (cname = "zwp_idle_inhibit_manager_v1_create_inhibitor")]
        public IdleInhibitorV1 create_inhibitor(Wl.Surface surface);
    }

    [CCode (cname = "struct zwp_idle_inhibitor_v1", free_function = "zwp_idle_inhibitor_v1_destroy")]
    [Compact]
    public class IdleInhibitorV1 : Wl.Proxy {
        public void set_user_data (void* user_data);
        public void* get_user_data ();
        public uint32 get_version ();
    }

    [CCode (cname = "zwp_idle_inhibit_manager_v1_interface")]
    public static Wl.Interface idle_inhibit_manager_v1_interface;
}

// Extensions to Wl namespace for missing interfaces
[CCode (cheader_filename = "wayland-client-protocol.h")]
namespace Wl {
    [CCode (cname = "wl_compositor_interface")]
    public static Interface compositor_interface;
}
