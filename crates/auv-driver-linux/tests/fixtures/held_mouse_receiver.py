"""Independent GTK4 receipt process for the opt-in held-mouse test."""
import sys
import gi

gi.require_version("Gtk", "4.0")
from gi.repository import GLib, Gtk


def emit(*parts):
    print(*parts, flush=True)


app = Gtk.Application(application_id="ai.moeru.auv.held-receiver")


def activate(app):
    window = Gtk.ApplicationWindow(application=app, title="AUV held mouse receiver")
    area = Gtk.DrawingArea()
    window.set_child(area)
    was_active = False

    def active_changed(window, _property):
        nonlocal was_active
        if window.is_active():
            was_active = True
        elif was_active:
            emit("inactive")

    window.connect("notify::is-active", active_changed)
    click = Gtk.GestureClick()
    click.set_button(0)
    click.connect("pressed", lambda g, n, x, y: emit("down", g.get_current_button(), x, y))
    click.connect("released", lambda g, n, x, y: emit("up", g.get_current_button(), x, y))
    area.add_controller(click)
    motion = Gtk.EventControllerMotion()
    motion.connect("motion", lambda g, x, y: emit("move", int(g.get_current_event_state()), x, y))
    area.add_controller(motion)

    def command(source, condition):
        line = sys.stdin.readline()
        if not line:
            app.quit()
            return False
        # Readiness is queried before each gesture, through the receiving app.
        if line.strip() == "status":
            emit("status", int(window.is_active()))
        return True

    GLib.io_add_watch(sys.stdin, GLib.IO_IN | GLib.IO_HUP, command)
    GLib.timeout_add_seconds(90, lambda: (app.quit(), False)[1])
    window.fullscreen()
    window.present()
    emit("ready")


app.connect("activate", activate)
app.run([])
