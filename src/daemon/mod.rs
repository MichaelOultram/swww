use image::imageops::FilterType;
use log::{debug, error, info};
use simplelog::{ColorChoice, LevelFilter, TermLogger, TerminalMode};

use smithay_client_toolkit::{
    environment::Environment,
    output::{with_output_info, OutputInfo},
    reexports::{
        calloop::{self, channel},
        client::protocol::{wl_output, wl_shm, wl_surface},
        client::{Attached, Display, EventQueue, Main},
        protocols::wlr::unstable::layer_shell::v1::client::{
            zwlr_layer_shell_v1, zwlr_layer_surface_v1,
        },
    },
    shm::MemPool,
    WaylandSource,
};

use std::{
    cell::{Cell, RefCell, RefMut},
    fs,
    io::{Read, Write},
    os::unix::net::UnixListener,
    path::{Path, PathBuf},
    rc::Rc,
    str::FromStr,
};

use crate::cli::Fswww;

mod processor;
use processor::ProcessorResult;
mod wayland;

#[derive(PartialEq, Copy, Clone)]
enum RenderEvent {
    Configure { width: u32, height: u32 },
    Closed,
}

struct Background {
    output_name: String,
    surface: wl_surface::WlSurface,
    layer_surface: Main<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1>,
    next_render_event: Rc<Cell<Option<RenderEvent>>>,
    pool: MemPool,
    dimensions: (u32, u32),
    img: Option<PathBuf>,
}

impl Background {
    fn new(
        output: &wl_output::WlOutput,
        output_name: String,
        surface: wl_surface::WlSurface,
        layer_shell: &Attached<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
        pool: MemPool,
    ) -> Option<Self> {
        let layer_surface = layer_shell.get_layer_surface(
            &surface,
            Some(output),
            zwlr_layer_shell_v1::Layer::Background,
            "fswww".to_owned(),
        );

        layer_surface.set_anchor(zwlr_layer_surface_v1::Anchor::all());
        layer_surface.set_exclusive_zone(-1);

        let next_render_event = Rc::new(Cell::new(None::<RenderEvent>));
        let next_render_event_handle = Rc::clone(&next_render_event);
        layer_surface.quick_assign(move |layer_surface, event, _| {
            match (event, next_render_event_handle.get()) {
                (zwlr_layer_surface_v1::Event::Closed, _) => {
                    next_render_event_handle.set(Some(RenderEvent::Closed));
                }
                (
                    zwlr_layer_surface_v1::Event::Configure {
                        serial,
                        width,
                        height,
                    },
                    next,
                ) if next != Some(RenderEvent::Closed) => {
                    layer_surface.ack_configure(serial);
                    next_render_event_handle.set(Some(RenderEvent::Configure { width, height }));
                }
                (_, _) => {}
            }
        });

        // Commit so that the server will send a configure event
        surface.commit();

        Some(Self {
            surface,
            layer_surface,
            next_render_event,
            pool,
            output_name,
            dimensions: (0, 0),
            img: None,
        })
    }

    /// Handles any events that have occurred since the last call, redrawing if needed.
    /// Returns true if the surface should be dropped.
    fn handle_events(&mut self) -> bool {
        match self.next_render_event.take() {
            Some(RenderEvent::Closed) => true,
            Some(RenderEvent::Configure { width, height }) => {
                self.pool.resize((width * height * 4) as usize).unwrap();
                self.dimensions = (width, height);
                false
            }
            None => false,
        }
    }

    fn draw(&mut self, img: &[u8]) {
        let stride = 4 * self.dimensions.0 as i32;
        let width = self.dimensions.0 as i32;
        let height = self.dimensions.1 as i32;

        let buffer = self
            .pool
            .buffer(0, width, height, stride, wl_shm::Format::Argb8888);
        let canvas = self.pool.mmap();

        canvas.copy_from_slice(img);
        info!("Copied img to buffer.");

        // Attach the buffer to the surface and mark the entire surface as damaged
        self.surface.attach(Some(&buffer), 0, 0);
        self.surface
            .damage_buffer(0, 0, width as i32, height as i32);

        // Finally, commit the surface
        self.surface.commit();
    }

    ///Same as draw, but decompresses the sent image first
    fn animate(&mut self, frame: &[u8]) {
        let stride = 4 * self.dimensions.0 as i32;
        let width = self.dimensions.0 as i32;
        let height = self.dimensions.1 as i32;

        let buffer = self
            .pool
            .buffer(0, width, height, stride, wl_shm::Format::Argb8888);
        let canvas = self.pool.mmap();
        processor::comp_decomp::mixed_decomp(canvas, frame);
        info!("Decompressed frame.");

        // Attach the buffer to the surface and mark the entire surface as damaged
        self.surface.attach(Some(&buffer), 0, 0);
        self.surface
            .damage_buffer(0, 0, width as i32, height as i32);

        // Finally, commit the surface
        self.surface.commit();
    }

    fn get_current_img(&mut self) -> Option<Vec<u8>> {
        if self.img.is_some() {
            let (width, height) = self.dimensions;
            let mut img = vec![0; (width * height * 4) as usize];
            img.copy_from_slice(self.pool.mmap());
            Some(img)
        } else {
            None
        }
    }
}

impl Drop for Background {
    fn drop(&mut self) {
        self.layer_surface.destroy();
        self.surface.destroy();
    }
}

pub fn main() {
    let listener = make_socket(); //Must make this first because the file we log to is in there

    make_logger();
    debug!(
        "Made socket in {:?} and initalized logger. Starting daemon...",
        listener.local_addr().unwrap()
    );

    let (env, display, queue) = wayland::make_wayland_environment();

    let mut bgs = Rc::new(RefCell::new(Vec::new()));

    let layer_shell = env.require_global::<zwlr_layer_shell_v1::ZwlrLayerShellV1>();

    let env_handle = env.clone();
    let bgs_handle = Rc::clone(&bgs);
    let output_handler = move |output: wl_output::WlOutput, info: &OutputInfo| {
        create_backgrounds(output, info, &env_handle, &bgs_handle, &layer_shell.clone())
    };
    // Process currently existing outputs
    for output in env.get_all_outputs() {
        if let Some(info) = with_output_info(&output, Clone::clone) {
            output_handler(output, &info);
        }
    }

    // Setup a listener for changes
    // The listener will live for as long as we keep this handle alive
    let _listner_handle =
        env.listen_for_outputs(move |output, info, _| output_handler(output, info));

    //NOTE: we can't move these into the function because it causes a segfault
    run_main_loop(&mut bgs, queue, &display, listener);
    info!("Finished running event loop. Exiting...");
}

fn make_logger() {
    #[cfg(debug_assertions)]
    {
        let config = simplelog::ConfigBuilder::new()
            .set_thread_level(LevelFilter::Info) //let me see where the processing is happenning
            .set_time_format_str("%H:%M:%S%.f") //let me see those nanoseconds
            .build();
        TermLogger::init(
            LevelFilter::Debug,
            config,
            TerminalMode::Stderr,
            ColorChoice::AlwaysAnsi,
        )
        .expect("Failed to initialize logger. Cancelling...");
    }

    #[cfg(not(debug_assertions))]
    {
        TermLogger::init(
            LevelFilter::Warn,
            simplelog::Config::default(),
            TerminalMode::Stderr,
            ColorChoice::Auto,
        )
        .expect("Failed to initialize logger. Cancelling...");
    }
}

fn create_backgrounds(
    output: wl_output::WlOutput,
    info: &OutputInfo,
    env: &Environment<wayland::Env>,
    bgs: &Rc<RefCell<Vec<Background>>>,
    layer_shell: &Attached<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
) {
    if info.obsolete {
        // an output has been removed, release it
        bgs.borrow_mut().retain(|bg| bg.output_name != info.name);
        output.release();
    } else {
        // an output has been created, construct a surface for it
        let surface = env.create_surface().detach();
        let pool = env
            .create_simple_pool(|_| {})
            .expect("Failed to create a memory pool!");

        debug!("New background with output: {:?}", info);
        if let Some(bg) = Background::new(&output, info.name.clone(), surface, layer_shell, pool) {
            (*bgs.borrow_mut()).push(bg);
        }
    }
}

fn make_socket() -> UnixListener {
    let runtime_dir = if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        dir
    } else {
        "/tmp/fswww".to_string()
    };

    let runtime_dir = Path::new(&runtime_dir);

    if !runtime_dir.exists() {
        fs::create_dir(runtime_dir).expect("Failed to create runtime dir...");
    }

    let socket = runtime_dir.join("fswww.socket");

    UnixListener::bind(socket).expect("Couldn't bind socket")
}

///bgs and display can't be moved into here because it causes a segfault
fn run_main_loop(
    bgs: &mut Rc<RefCell<Vec<Background>>>,
    queue: EventQueue,
    display: &Display,
    listener: UnixListener,
) {
    let (frame_sender, frame_receiver) = calloop::channel::channel();
    let mut processor = processor::Processor::new(frame_sender);
    let mut event_loop = calloop::EventLoop::<calloop::LoopSignal>::try_new().unwrap();
    let event_handle = event_loop.handle();

    event_handle
        .insert_source(frame_receiver, |evt, _, loop_signal| match evt {
            channel::Event::Msg(msg) => handle_recv_frame(&mut bgs.borrow_mut(), &msg),
            channel::Event::Closed => loop_signal.stop(),
        })
        .unwrap();

    listener.set_nonblocking(true).unwrap();
    event_handle
        .insert_source(
            calloop::generic::Generic::new(listener, calloop::Interest::READ, calloop::Mode::Level),
            |_, listener, loop_signal| {
                recv_socket_msg(bgs.borrow_mut(), listener, loop_signal, &mut processor)
            },
        )
        .unwrap();

    WaylandSource::new(queue)
        .quick_insert(event_handle)
        .unwrap();

    //IMPORTANT: For here on out, any failures must NOT result in a panic. We need to exit cleanly.
    //If it's unrecoverable, we should also delete the socket. Note that on normal exit the cleanup
    //happens at the calling fswww instance (because we can't send back an answer after we've
    //removed the socket. So we can only assure the user the socket has been removed in the fswww
    //client).

    let mut loop_signal = event_loop.get_signal();
    if let Err(e) = event_loop.run(None, &mut loop_signal, |_| {
        {
            let mut bgs = bgs.borrow_mut();
            let mut i = 0;
            while i != bgs.len() {
                if bgs[i].handle_events() {
                    bgs.remove(i);
                } else {
                    i += 1;
                }
            }
        }
        if let Err(e) = display.flush() {
            error!("Couldn't flush display: {}", e);
        }
    }) {
        error!("Event loop closed unexpectedly: {}", e);
    }
}

fn recv_socket_msg(
    mut bgs: RefMut<Vec<Background>>,
    listener: &UnixListener,
    loop_signal: &calloop::LoopSignal,
    processor: &mut processor::Processor,
) -> Result<calloop::PostAction, std::io::Error> {
    match listener.accept() {
        Ok((mut socket, _)) => {
            let mut buf = String::with_capacity(100);
            if let Err(e) = socket.read_to_string(&mut buf) {
                error!("Failed to read socket: {}", e);
                return Err(e);
            };
            let request = Fswww::from_str(&buf);
            let mut answer = Ok(String::new());
            match request {
                Ok(Fswww::Kill) => loop_signal.stop(),
                Ok(Fswww::Img(img)) => {
                    let outputs = get_real_outputs(&mut bgs, &img.outputs);
                    if outputs.is_empty() {
                        answer = Err("None of the outputs sent are valid.".to_string());
                        send_answer(answer, &listener);
                        return Ok(calloop::PostAction::Continue);
                    }
                    let filter = get_filter_from_str(&img.filter.to_string());
                    for result in
                        send_request_to_processor(&mut bgs, outputs, filter, &img.path, processor)
                    {
                        debug!("Received img as processing result");
                        handle_recv_img(&mut bgs, &result, &img.path);
                    }
                }
                Ok(Fswww::Init { .. }) => (),
                Ok(Fswww::Query) => answer = Ok(outputs_name_and_dim(&mut bgs)),
                Err(e) => answer = Err(e),
            }
            send_answer(answer, &listener);
            Ok(calloop::PostAction::Continue)
        }
        Err(e) => Err(e),
    }
}

///Verifies that all outputs exist
///Also puts in all outpus if an empty string was offered
fn get_real_outputs(bgs: &mut RefMut<Vec<Background>>, outputs: &str) -> Vec<String> {
    let mut real_outputs: Vec<String> = Vec::with_capacity(bgs.len());
    //An empty line means all outputs
    if outputs.is_empty() {
        for bg in bgs.iter() {
            real_outputs.push(bg.output_name.to_owned());
        }
    } else {
        for output in outputs.split(',') {
            let output = output.to_string();
            let mut exists = false;
            for bg in bgs.iter() {
                if output == bg.output_name {
                    exists = true;
                }
            }

            if !exists {
                error!("Output {} does not exist!", output);
            } else if !real_outputs.contains(&output) {
                real_outputs.push(output);
            }
        }
    }
    debug!("Requesting img for outputs: {:?}", real_outputs);
    real_outputs
}

///Returns one result per output with same dimesions and image
fn send_request_to_processor(
    bgs: &mut RefMut<Vec<Background>>,
    mut outputs: Vec<String>,
    filter: FilterType,
    img: &Path,
    processor: &mut processor::Processor,
) -> Vec<ProcessorResult> {
    let mut processing_results = Vec::new();

    while !outputs.is_empty() {
        let mut out_same_dim = Vec::with_capacity(outputs.len());
        out_same_dim.push(outputs.pop().unwrap());

        let dim;
        let old_img_path;
        let old_img;
        {
            let bg = bgs
                .iter_mut()
                .find(|bg| bg.output_name == out_same_dim[0])
                .unwrap();
            dim = bg.dimensions;
            old_img_path = bg.img.clone();
            old_img = bg.get_current_img();
        }

        for bg in bgs.iter().filter(|bg| outputs.contains(&bg.output_name)) {
            if bg.dimensions == dim && bg.img == old_img_path {
                out_same_dim.push(bg.output_name.clone());
            }
        }
        outputs.retain(|o| !out_same_dim.contains(o));

        debug!(
            "Sending message to processor: {:?}",
            (&out_same_dim, dim, img.to_path_buf())
        );
        let result = processor.process((out_same_dim, dim, filter, img, old_img));
        processing_results.push(result);
    }

    processing_results
}

fn get_filter_from_str(s: &str) -> FilterType {
    match s {
        "Nearest" => FilterType::Nearest,
        "Triangle" => FilterType::Triangle,
        "CatmullRom" => FilterType::CatmullRom,
        "Gaussian" => FilterType::Gaussian,
        "Lanczos3" => FilterType::Lanczos3,
        _ => unreachable!(), //This is impossible because we test it before sending
    }
}

fn handle_recv_img(
    bgs: &mut RefMut<Vec<Background>>,
    msg: &(Vec<String>, Vec<u8>),
    img_path: &Path,
) {
    let (outputs, img) = msg;
    for bg in bgs.iter_mut() {
        if outputs.contains(&bg.output_name) {
            bg.img = Some(img_path.to_path_buf());
            bg.draw(img);
        }
    }
}

fn handle_recv_frame(bgs: &mut RefMut<Vec<Background>>, msg: &(Vec<String>, Vec<u8>)) {
    let (outputs, frame) = msg;
    for bg in bgs.iter_mut() {
        if outputs.contains(&bg.output_name) {
            bg.animate(frame);
        }
    }
}

fn outputs_name_and_dim(bgs: &mut RefMut<Vec<Background>>) -> String {
    let mut str = String::new();
    for bg in bgs.iter() {
        str += &format!(
            "{} - Dimensions: {}x{}\n",
            bg.output_name, bg.dimensions.0, bg.dimensions.1
        );
    }
    str
}

fn send_answer(answer: Result<String, String>, listener: &UnixListener) {
    let mut socket;

    match listener.accept() {
        Ok((s, _)) => socket = s,
        Err(e) => {
            error!(
                "Failed to get socket stream while sending answer back: {}",
                e
            );
            return ();
        }
    }

    let send_result = match answer {
        Ok(msg) => socket.write_all(format!("Ok\n{}", msg).as_bytes()),
        Err(err) => socket.write_all(format!("Err\n{}", err).as_bytes()),
    };
    if let Err(e) = send_result {
        error!("Error sending answer back: {}", e);
    }
}