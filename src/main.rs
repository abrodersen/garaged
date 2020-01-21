
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

use std::os::raw::c_int;
use std::process::exit;
use std::thread::sleep;
use std::time::Duration;

use parking_lot::{Mutex, Condvar};

static SIGNAL: Condvar = Condvar::new();

fn main() {
    exit(run());
}

fn run() -> i32  {
    
    println!("initializing ionoPi");
    if unsafe { ionoPiSetup() } == 0 {
        eprintln!("error: failed to initialize libionoPi");
        return 1;
    }
    
    println!("resetting relay");
    unsafe { ionoPiDigitalWrite(O1, OPEN) };

    println!("resetting led");
    unsafe { ionoPiDigitalWrite(LED, OFF) };
    
    println!("registering for input changes");
    if unsafe { ionoPiDigitalInterrupt(DI4, INT_EDGE_RISING, Some(digital_input_callback)) } == 0 {
        eprintln!("error: failed to create digital input interrupt");
        return 1;
    }

    let mutex = Mutex::new(());

    println!("beginning monitor loop");
    loop {
        SIGNAL.wait(&mut mutex.lock());
        println!("received signal");

        for _ in 0..5 {
            unsafe { ionoPiDigitalWrite(LED, OFF) };

            sleep(Duration::from_millis(50));

            unsafe { ionoPiDigitalWrite(LED, ON) };

            sleep(Duration::from_millis(50));
        }

        println!("closing relay");
        unsafe { ionoPiDigitalWrite(O1, CLOSED) };

        sleep(Duration::from_millis(250));

        println!("opening relay");
        unsafe { ionoPiDigitalWrite(O1, OPEN) };

        sleep(Duration::from_millis(250));
    }
}

#[no_mangle]
pub extern "C" fn digital_input_callback(_line: c_int, state: c_int) {
    if state == HIGH {
        SIGNAL.notify_all();
    }
}
