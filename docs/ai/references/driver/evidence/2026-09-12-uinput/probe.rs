use auv_driver_common::{Driver,geometry::Point,input::{Click,ClickModifiers,KeyPressOptions,Scroll}};
use auv_driver_linux::{LinuxDriver,InputBackend};
fn main()->Result<(),Box<dyn std::error::Error>>{
 let s=LinuxDriver::new().with_input_backend(InputBackend::Uinput).open_local()?;
 println!("displays {:?}",s.display().list()?);
 for key in ["a","shift+b","exclam"] {println!("key {key}: {:?}",s.input().press_key(KeyPressOptions{key:key.into(),..Default::default()}));std::thread::sleep(std::time::Duration::from_millis(100));}
 println!("click {:?}",s.input().click_at(Point::new(600.,400.),Click::Single,ClickModifiers{shift:true,control:true,..Default::default()}));
 println!("scroll {:?}",s.input().scroll_at(Point::new(600.,400.),Scroll{delta_x:0.,delta_y:60.},std::time::Duration::from_millis(100)));
 std::thread::sleep(std::time::Duration::from_secs(1));Ok(())
}
