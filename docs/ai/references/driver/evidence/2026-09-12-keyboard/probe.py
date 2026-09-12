import subprocess,json,os,time
env=dict(os.environ,XDG_RUNTIME_DIR='/run/user/1000',WAYLAND_DISPLAY='wayland-0',DBUS_SESSION_BUS_ADDRESS='unix:path=/run/user/1000/bus',AUV_LINUX_INPUT_BACKEND='uinput')
auv='/tmp/auv-click-modifiers.6uouEh/target/debug/auv'
requests=[['input.key','a'],['input.keys','shift','b','--count','2','--interval-ms','100'],['input.keyboard','--actions',json.dumps([{'kind':'press','keys':['c']},{'kind':'type_text','text':'d!'}])],['input.keyboard','--actions',json.dumps([{'kind':'press','keys':['x']},{'kind':'press','keys':['invalid-key']}])]]
for req in requests:
 p=subprocess.run([auv,'invoke',*req,'--json','--store-root','/tmp/auv-keyboard-live-runs'],env=env,capture_output=True,text=True)
 print(p.stdout,flush=True)
 if p.stderr: print(p.stderr,flush=True)
 time.sleep(.2)
