use std::{
    env,
    fs::File,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use gumdrop::Options;
use log::info;
use opencv::{
    core::{Mat, Vector, VectorToVec},
    imgcodecs,
};
use rocket::{
    fs::{FileServer, NamedFile},
    get,
    http::{ContentType, Status},
    post,
    response::{stream::ByteStream, Redirect},
    tokio::time::sleep,
    Request, State,
};
use svled::{load_validate_conf, Config, GetEventsFrameBuffer, ManagerData, UnityOptions};

#[macro_use]
extern crate rocket;

pub mod opensauce;

#[derive(Debug, Options)]
struct MyOptions {
    #[options(help = "print help message")]
    help: bool,
    #[options(help = "start webserver")]
    serve: bool,
    #[options(help = "start the scramble demo")]
    scramble: bool,
}

#[derive(Clone)]
pub struct SvledManagerHolder {
    pub manager: Arc<Mutex<ManagerData>>,
    pub unity_options: UnityOptions,
    pub config_holder_svled: Config,
    pub frame_buffer: Option<Arc<Mutex<GetEventsFrameBuffer>>>,
    pub frame_source: Option<Arc<Mutex<bool>>>,
    pub restart_calib_success: Arc<Mutex<bool>>,
}

#[get("/")]
async fn index() -> Result<NamedFile, std::io::Error> {
    NamedFile::open("static/index.html").await
}

#[get("/scramble-demo", format = "text/html")]
async fn scramble_demo_path() -> Result<NamedFile, std::io::Error> {
    NamedFile::open("static/scramble.html").await
}

#[post("/start-scramble")]
async fn scramble_demo_rocket(manager: &State<Arc<Mutex<SvledManagerHolder>>>) -> &'static str {
    let manager_clone = Arc::clone(manager.inner());

    info!("Starting scramble thread");
    std::thread::spawn(move || {
        let (crop_override_tuple, unity_options, config_holder, manager_ref) = {
            let mgr = manager_clone.lock().unwrap();

            let crop_override_tuple = mgr
                .config_holder_svled
                .advanced
                .transform
                .crop_override
                .as_ref()
                .map(|v| ((v[0], v[1], v[2], v[3]), (v[4], v[5], v[6], v[7])));

            (
                crop_override_tuple,
                mgr.unity_options.clone(),
                mgr.config_holder_svled.clone(),
                Arc::clone(&mgr.manager),
            )
        };

        info!("Starting scramble_demo");

        opensauce::scramble_demo(
            &crop_override_tuple,
            &manager_ref,
            &unity_options,
            &config_holder,
            Some(&manager_clone),
        );
    });

    "Scramble started"
}

#[get("/video-cam-1")]
async fn video_stream_cam_1(
    manager: &State<Arc<Mutex<SvledManagerHolder>>>,
) -> (ContentType, ByteStream![Vec<u8>]) {
    let manager = Arc::clone(manager.inner());

    let stream = ByteStream! {
        loop {
            let frame = { // TODO: This is bad
                let mgr_lock = match manager.lock() {
                    Ok(lock) => lock,
                    Err(_) => break,
                };
                if mgr_lock.frame_source.is_some() {
                    if *mgr_lock.frame_source.as_ref().unwrap().lock().unwrap() {
                        mgr_lock.manager.lock().unwrap().vision.frame_cam_1.clone()
                    } else {
                        mgr_lock.frame_buffer.as_ref().unwrap().lock().unwrap().shared_frame_1.clone()
                    }
                } else {
                    mgr_lock.manager.lock().unwrap().vision.frame_cam_1.clone()
                }
            };

            let mut buf = Vector::new();
            if imgcodecs::imencode(".jpg", &frame, &mut buf, &Vector::new()).is_err() {
                continue;
            }
            let jpeg = buf.to_vec();

            yield format!(
                "--frame\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n",
                jpeg.len()
            ).into_bytes();
            yield jpeg;
            yield b"\r\n".to_vec();

            sleep(Duration::from_millis(33)).await;
        }
    };

    (
        ContentType::new("multipart", "x-mixed-replace; boundary=frame"),
        stream,
    )
}

#[get("/video-cam-2")]
async fn video_stream_cam_2(
    // TODO functionize
    manager: &State<Arc<Mutex<SvledManagerHolder>>>,
) -> (ContentType, ByteStream![Vec<u8>]) {
    let manager = Arc::clone(manager.inner());

    let stream = ByteStream! {
        loop {
            let frame = {
                let mgr_lock = match manager.lock() {
                    Ok(lock) => lock,
                    Err(_) => break,
                };
                if mgr_lock.frame_source.is_some() {
                    if *mgr_lock.frame_source.as_ref().unwrap().lock().unwrap() {
                        mgr_lock.manager.lock().unwrap().vision.frame_cam_2.clone()
                    } else {
                        mgr_lock.frame_buffer.as_ref().unwrap().lock().unwrap().shared_frame_2.clone()
                    }
                } else {
                    mgr_lock.manager.lock().unwrap().vision.frame_cam_2.clone()
                }
            };

            let mut buf = Vector::new();
            if imgcodecs::imencode(".jpg", &frame, &mut buf, &Vector::new()).is_err() {
                continue;
            }
            let jpeg = buf.to_vec();

            yield format!(
                "--frame\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n",
                jpeg.len()
            ).into_bytes();
            yield jpeg;
            yield b"\r\n".to_vec();

            sleep(Duration::from_millis(33)).await;
        }
    };

    (
        ContentType::new("multipart", "x-mixed-replace; boundary=frame"),
        stream,
    )
}

#[get("/error", format = "text/html")]
async fn error() -> Result<NamedFile, std::io::Error> {
    NamedFile::open("static/error.html").await
}

#[post("/recalibrate")]
fn recalibrate() -> Status {
    match File::create("end_loop") {
        Ok(_) => Status::Ok,
        Err(e) => {
            error!("Failed to create file: {}", e);
            Status::InternalServerError
        }
    }
}

#[get("/recalibrate_success")]
fn recalibrate_success(manager: &State<Arc<Mutex<SvledManagerHolder>>>) -> String {
    let manager = Arc::clone(manager.inner());

    match *manager
        .lock()
        .unwrap()
        .restart_calib_success
        .lock()
        .unwrap()
    {
        true => return "SUCCESS".to_string(),
        false => return "FAIL".to_string(),
    };
}

#[catch(500)]
fn internal_error(_req: &Request) -> Redirect {
    Redirect::to(uri!("/error"))
}

#[rocket::main]
async fn main() -> Result<(), Box<rocket::Error>> {
    let opts = MyOptions::parse_args_default_or_exit();

    if opts.help {
        println!("Usage: --serve or --scramble");
        return Ok(());
    }

    env::set_var("RUST_LOG", "info");
    env_logger::init();

    let config_load_result = load_validate_conf(Path::new("svled.toml"));

    let (manager, unity_options, config_holder) = (
        Arc::new(Mutex::new(config_load_result.0)),
        config_load_result.1,
        config_load_result.2,
    );

    let svled_manager = Arc::new(Mutex::new(SvledManagerHolder {
        manager,
        unity_options,
        config_holder_svled: config_holder,
        frame_buffer: None,
        frame_source: None,
        restart_calib_success: Arc::new(Mutex::new(true)),
    }));

    if opts.scramble {
        scramble_demo_rocket(State::from(&svled_manager.clone())).await;
        return Ok(());
    }

    if opts.serve {
        info!("Starting webserver!");

        info!("Initializing svled...");
        let config_load_result = load_validate_conf(Path::new("svled.toml"));

        let (manager_svled, unity_options, config_holder_svled) = (
            Arc::new(Mutex::new(config_load_result.0)),
            config_load_result.1,
            config_load_result.2,
        );

        // Set default image for /video
        manager_svled.lock().unwrap().vision.frame_cam_1 =
            imgcodecs::imread("static/images/hatred.jpg", imgcodecs::IMREAD_COLOR).unwrap();

        manager_svled.lock().unwrap().vision.frame_cam_2 =
            imgcodecs::imread("static/images/hatred.jpg", imgcodecs::IMREAD_COLOR).unwrap();

        let svled_manager = Arc::new(Mutex::new(SvledManagerHolder {
            manager: manager_svled,
            unity_options,
            config_holder_svled,
            frame_buffer: Some(Arc::new(Mutex::new(GetEventsFrameBuffer {
                shared_frame_1: Mat::default(),
                shared_frame_2: Mat::default(),
            }))),
            frame_source: Some(Arc::new(Mutex::new(true))),
            restart_calib_success: Arc::new(Mutex::new(true)),
        }));

        let rocket = rocket::build()
            .mount(
                "/",
                routes![
                    index,
                    error,
                    video_stream_cam_1,
                    video_stream_cam_2,
                    scramble_demo_path,
                    scramble_demo_rocket,
                    recalibrate,
                    recalibrate_success
                ],
            )
            .mount("/static", FileServer::from("./static"))
            .register("/", catchers![internal_error])
            .manage(svled_manager);

        rocket.launch().await?;
    }

    Ok(())
}
