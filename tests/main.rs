#![feature(deadline_api)]

extern crate serial;

use serial::prelude::*;
use std::collections::HashMap;
use std::io;
use std::io::prelude::*;
use std::process::Command;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

pub type ResultMap = Arc<Mutex<HashMap<String, String>>>;
const TEST_RESULT_START: &'static str = "[test-results]\n";

#[test]
fn main_test() {
    let mut result_map: ResultMap = Arc::new(Mutex::new(HashMap::new()));
    let result_map_parser = Arc::clone(&result_map);
    let result_map_main = result_map.clone();

    let (send_message_parsed, receive_message_parsed) = mpsc::channel();
    let (send_no_more_data, receive_no_more_data) = mpsc::channel();
    let (send_test_results, receive_test_results) = mpsc::channel();

    install_app_onto_board();

    let serial_port = thread::spawn(move || {
        let mut port = serial::open("/dev/ttyACM0").unwrap();

        port.reconfigure(&|settings| {
            settings.set_baud_rate(serial::Baud115200).unwrap();
            settings.set_char_size(serial::Bits8);
            settings.set_parity(serial::ParityNone);
            settings.set_stop_bits(serial::Stop1);
            settings.set_flow_control(serial::FlowNone);
            Ok(())
        });
        port.set_timeout(Duration::from_millis(500)).is_err();

        loop {
            let mut buf: Vec<u8> = [0; 1024].to_vec();

            if receive_no_more_data.try_recv().is_ok() {
                break;
            }
            port.read(&mut buf[..]).is_err();

            let filtered_buffer = buf.to_vec()
                .into_iter()
                .filter(|&x| x != 0)
                .collect::<Vec<u8>>();
            send_message_parsed
                .send(String::from_utf8_lossy(&filtered_buffer).into_owned())
                .is_err();
        }
    });

    let parser = thread::spawn(move || {
        let mut buf = String::new();
        let mut report_started = false;

        loop {
            let mut data = Mutex::lock(&result_map_parser).unwrap();

            buf += &receive_message_parsed.recv().unwrap();
            match buf.rfind(TEST_RESULT_START) {
                Some(index) => {
                    buf.drain(0..index + TEST_RESULT_START.len());
                    report_started = true;
                }
                None => (),
            }

            match data.get(&String::from("test")) {
                Some(value) => {
                    if value == "\"done\"" {
                        send_no_more_data.send(()).unwrap();
                        send_test_results.send(()).unwrap();
                        break;
                    }
                }
                None => (),
            }

            if report_started {
                put_into_map(&mut data, &mut buf);
            }
        }
    });

    receive_test_results
        .recv_deadline(Instant::now() + Duration::from_secs(10))
        .unwrap();

    println!("Test results: \n");
    let data = result_map_main.lock().unwrap();
    for (key, value) in data.iter() {
        println!("Key: {}, Value: {}", key, value);
    }
    assert_eq!(
        data.get(&String::from("heap_test")),
        Some(&String::from("\"Heap works.\""))
    );
    assert_eq!(
        data.get(&String::from("test_ipc")),
        Some(&String::from("\"passed\""))
    );
    println!("Successful!\n");

    serial_port.join().unwrap();
    parser.join().unwrap();
}

fn install_app_onto_board() {
    let output = Command::new("sh")
        .arg("run_hardware_test.sh")
        .current_dir("libtock-rs")
        .output()
        .expect("Error running run_example script.");

    if !output.status.success() {
        println!("stdout: {}", String::from_utf8_lossy(&output.stdout));
        println!("stderr: {}", String::from_utf8_lossy(&output.stderr));
        panic!();
    }
}

fn put_into_map(data: &mut HashMap<String, String>, buf: &mut String) {
    match buf.rfind("\n") {
        Some(index) => {
            {
                let (before, _) = buf.split_at(index);
                match before.find("=") {
                    Some(index) => {
                        let (key, _) = before.split_at(index);
                        let (_, value) = before.split_at(index + 1);
                        data.insert(String::from(key.trim()), String::from(value.trim()));
                    }
                    None => (),
                }
            }
            buf.drain(0..index);
        }
        None => (),
    }
}
