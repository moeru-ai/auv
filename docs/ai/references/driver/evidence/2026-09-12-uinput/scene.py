import gi,json,time
gi.require_version('Gtk','3.0')
from gi.repository import Gtk,Gdk
log=open('/tmp/auv-uinput-scene.jsonl','w',buffering=1)
def record(kind,**values): log.write(json.dumps(dict(time=time.time(),kind=kind,**values))+'\n')
w=Gtk.Window(title='AUV input validation'); w.set_default_size(900,600)
b=Gtk.Box(orientation=Gtk.Orientation.VERTICAL,spacing=20);w.add(b)
b.pack_start(Gtk.Label(label='AUV dedicated input validation — safe test window'),False,False,20)
e=Gtk.Entry();b.pack_start(e,False,False,10)
e.connect('changed',lambda e:record('text',text=e.get_text()))
def key(w,event):record('key',key=Gdk.keyval_name(event.keyval),state=int(event.state),pressed=event.type==Gdk.EventType.KEY_PRESS);return False
w.connect('key-press-event',key);w.connect('key-release-event',key)
area=Gtk.EventBox();area.add(Gtk.Label(label='Click and scroll test area'));b.pack_start(area,True,True,0)
area.add_events(Gdk.EventMask.BUTTON_PRESS_MASK|Gdk.EventMask.BUTTON_RELEASE_MASK|Gdk.EventMask.SCROLL_MASK|Gdk.EventMask.SMOOTH_SCROLL_MASK)
def pointer(w,event):record('pointer',event=str(event.type),x=event.x_root,y=event.y_root,state=int(event.state));return False
area.connect('button-press-event',pointer);area.connect('button-release-event',pointer);area.connect('scroll-event',pointer)
w.connect('destroy',Gtk.main_quit);w.show_all();w.maximize();e.grab_focus();w.present();record('ready');Gtk.main()
