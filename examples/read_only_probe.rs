//! Console-only, bounded diagnostics for the Daxa receiver.
//!
//! Run this example from a terminal after closing Daxa Control so the two
//! monitor instances do not compete for receiver replies. It never requests a
//! device configuration write.

#[allow(dead_code)]
#[path = "../src/device.rs"]
mod device;

use std::thread;
use std::time::{Duration, Instant};

use device::{
    BatteryState, ConnectionProbeState, DeviceMonitor, DeviceSnapshot, ReceiverState,
};

const PROBE_TIMEOUT: Duration = Duration::from_secs(20);
const POLL_INTERVAL: Duration = Duration::from_millis(100);

fn main() {
    println!("Daxa read-only device diagnostic");
    println!("Close Daxa Control first to avoid competing receiver queries.");
    println!("This diagnostic sends status and telemetry requests only.\n");

    let monitor = DeviceMonitor::start(eframe::egui::Context::default());
    let deadline = Instant::now() + PROBE_TIMEOUT;

    while Instant::now() < deadline {
        if let Some(snapshot) = monitor.try_latest() {
            print_snapshot(&snapshot);
            if is_terminal(&snapshot) {
                return;
            }
        }

        thread::sleep(POLL_INTERVAL);
    }

    println!("\nTimed out after 20 seconds before a final connection result.");
}

fn is_terminal(snapshot: &DeviceSnapshot) -> bool {
    match snapshot.receiver {
        ReceiverState::Scanning => false,
        ReceiverState::NotFound | ReceiverState::Unavailable => true,
        ReceiverState::Detected => snapshot.connection_probe != ConnectionProbeState::NotChecked,
    }
}

fn print_snapshot(snapshot: &DeviceSnapshot) {
    println!("Receiver: {:?}", snapshot.receiver);
    println!("Connection: {:?}", snapshot.connection_probe);

    if snapshot.vendor_collections.is_empty() {
        println!("Vendor HID collections: none");
    } else {
        println!("Vendor HID collections: {}", snapshot.vendor_collections.len());
        for collection in &snapshot.vendor_collections {
            println!(
                "  PID=0x{:04X}, usage_page=0x{:04X}, usage=0x{:04X}, interface={}",
                collection.product_id,
                collection.usage_page,
                collection.usage,
                collection.interface_number
            );
        }
    }

    if let Some(collection) = &snapshot.selected_collection {
        println!(
            "Selected collection: PID=0x{:04X}, usage_page=0x{:04X}, usage=0x{:04X}, interface={}",
            collection.product_id,
            collection.usage_page,
            collection.usage,
            collection.interface_number
        );
    } else {
        println!("Selected collection: none");
    }

    match snapshot.reported_profile_id {
        Some(profile_id) => println!("Mouse profile: {profile_id}"),
        None => println!("Mouse profile: unavailable"),
    }

    match &snapshot.battery {
        BatteryState::Available(reading) => println!(
            "Battery: {}%{}",
            reading.percent,
            if reading.charging { " (charging)" } else { "" }
        ),
        BatteryState::Ambiguous => println!("Battery: unavailable (multiple endpoints matched)"),
        BatteryState::Unavailable => println!("Battery: unavailable"),
    }

    println!();
}
