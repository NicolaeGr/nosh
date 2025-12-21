void clear_children (Gtk.Widget parent) {
    for (var child = parent.get_first_child (); child != null; child = parent.get_first_child ())
        child.unparent ();
}