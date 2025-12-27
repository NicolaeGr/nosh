using Gtk;
using Gdk;

namespace Notifications.Widgets {
    public class LookupIcon {
        private static Gtk.IconTheme? theme = null;

        private static Gtk.IconTheme get_theme () {
            if (theme == null) {
                var display = Display.get_default ();
                if (display != null) {
                    theme = IconTheme.get_for_display (display);
                }
            }
            return theme;
        }

        public static string? lookup (string name, string? fallback1 = null, string? fallback2 = null) {
            var icon_theme = get_theme ();
            if (icon_theme == null) {
                return name;
            }

            string?[] candidates = { name, fallback1, fallback2 };

            foreach (var candidate in candidates) {
                if (candidate == null || candidate.length == 0) {
                    continue;
                }

                if (icon_theme.has_icon (candidate)) {
                    return candidate;
                }

                var lower = candidate.down ();
                if (lower != candidate && icon_theme.has_icon (lower)) {
                    return lower;
                }

                var upper = candidate.up ();
                if (upper != candidate && icon_theme.has_icon (upper)) {
                    return upper;
                }
            }

            foreach (var candidate in candidates) {
                if (candidate != null && candidate.length > 0) {
                    return candidate;
                }
            }

            return "application-x-executable";
        }
    }
}
