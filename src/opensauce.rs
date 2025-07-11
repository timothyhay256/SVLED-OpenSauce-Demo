// This will contain any Open Sauce specific code, that way the rest can stay clean.
use std::{
    fs::remove_file,
    path::Path,
    sync::{Arc, Mutex},
    thread::{self, sleep},
    time::Duration,
};

use log::{debug, error, info};
use svled::{scan, signal_restart, unity, Config, ManagerData, UnityOptions};

use crate::SvledManagerHolder;

type CropData = Option<((i32, i32, i32, i32), (i32, i32, i32, i32))>;

pub fn scramble_demo(
    crop_data: &CropData,
    manager: &Arc<Mutex<ManagerData>>,
    unity_options: &UnityOptions,
    config_holder: &Config,
    svled_manager: Option<&Arc<Mutex<SvledManagerHolder>>>, // Holds frame buffer, and frame_source which is controlled by get_events.
) {
    info!("Starting scramble demo");
    let mut first_run = true;

    let signal_file = Path::new("./end_loop");
    if signal_file.exists() {
        remove_file(signal_file).expect("Unable to write signal file end_loop");
    }

    let cloned_manager = Arc::clone(manager);

    info!("Entering loop");

    loop {
        if first_run || signal_file.exists() {
            first_run = false;

            info!("Re-scanning");

            if signal_file.exists() {
                manager.lock().unwrap().state.keepalive = false;
                remove_file(signal_file).unwrap();
                info!("signal file exists, signaling restart");
                signal_restart(
                    config_holder.unity_options.unity_ip,
                    config_holder.unity_options.unity_ports[0],
                );
            }

            if let Some(manager) = svled_manager {
                if let Some(ref frame_source) = manager.lock().unwrap().frame_source {
                    *frame_source.lock().unwrap() = true;
                }
            }

            match scan(config_holder.clone(), manager, true, *crop_data) {
                Ok(_) => {
                    if let Some(manager) = svled_manager {
                        *manager
                            .lock()
                            .unwrap()
                            .restart_calib_success
                            .lock()
                            .unwrap() = true;
                    }
                    info!("Sending current positions to Unity");

                    match unity::send_pos(unity_options.clone()) {
                        Ok(_) => {}
                        Err(e) => {
                            panic!("There was an issue connecting to Unity: {e}")
                        }
                    };

                    info!("Starting listener thread");
                    if let Some(manager) = svled_manager {
                        if let Some(ref frame_source) = manager.lock().unwrap().frame_source {
                            *frame_source.lock().unwrap() = false;
                        }
                    }
                    let owned_options = unity_options.clone();
                    let owned_manager = Arc::clone(&cloned_manager);
                    let owned_config = config_holder.clone();

                    while !manager.lock().unwrap().state.keepalive {
                        // get_events will reset this once it has properly exited
                        sleep(Duration::new(0, 500000));
                    }

                    manager.lock().unwrap().state.keepalive = true;

                    let owned_frame_buffer = svled_manager.as_ref().and_then(|manager| {
                        manager
                            .lock()
                            .ok()
                            .and_then(|locked| locked.frame_buffer.as_ref().map(Arc::clone))
                    });

                    thread::spawn(move || {
                        debug!("inside thread");
                        match unity::get_events(
                            owned_manager,
                            &owned_options,
                            &owned_config,
                            &owned_options.unity_ports.clone()[0],
                            &owned_frame_buffer,
                        ) {
                            Ok(_) => {}
                            Err(e) => {
                                panic!("get_events thread crashed with error: {e}")
                            }
                        }
                    });
                }
                Err(e) => {
                    error!("could not scan with multiple cameras and streamlined enabled: {e}");
                    if let Some(manager) = svled_manager {
                        *manager
                            .lock()
                            .unwrap()
                            .restart_calib_success
                            .lock()
                            .unwrap() = false;
                    }
                }
            };
        }
    }
}

#[get("/")]
fn index() -> &'static str {
    "Hello, world!"
}

#[launch]
pub fn webserver() -> _ {
    info!("Starting rocket...");
    rocket::build().mount("/", routes![index])
}
