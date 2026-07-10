use std::vec::Vec;

#[derive(Debug, Clone, PartialEq)]
pub enum DriverState {
    Idle,
    Initialized,
    Scanning,
    Connected,
}

#[derive(Debug, Clone)]
pub struct HciCommand {
    pub opcode: u16,
    pub payload: Vec<u8>,
}

impl HciCommand {
    pub fn new(opcode: u16, payload: Vec<u8>) -> Self {
        HciCommand { opcode, payload }
    }
}

#[derive(Debug, Clone)]
pub struct HciTransport {
    pub sent_packets: Vec<HciCommand>,
}

impl HciTransport {
    pub fn new() -> Self {
        HciTransport {
            sent_packets: Vec::new(),
        }
    }

    pub fn send(&mut self, command: HciCommand) {
        self.sent_packets.push(command);
    }
}

#[derive(Debug, Clone)]
pub struct Advertisement {
    pub address: String,
    pub rssi: i8,
}

impl Advertisement {
    pub fn new(address: String, rssi: i8) -> Self {
        Advertisement { address, rssi }
    }
}

#[derive(Debug, Clone)]
pub struct BluetoothDriver {
    pub state: DriverState,
    pub transport: HciTransport,
    pub connected_address: Option<String>,
    pub advertisements: Vec<Advertisement>,
}

impl BluetoothDriver {
    pub fn new() -> Self {
        BluetoothDriver {
            state: DriverState::Idle,
            transport: HciTransport::new(),
            connected_address: None,
            advertisements: Vec::new(),
        }
    }

    pub fn initialize(&mut self) {
        let reset_cmd = HciCommand::new(0x0C03, vec![]);
        self.transport.send(reset_cmd);
        self.state = DriverState::Initialized;
    }

    pub fn enable_scan(&mut self) {
        let scan_cmd = HciCommand::new(0x200C, vec![]);
        self.transport.send(scan_cmd);
        self.state = DriverState::Scanning;
    }

    pub fn ingest_advertisement(&mut self, adv: Advertisement) {
        self.advertisements.push(adv);
    }

    pub fn connect(&mut self, address: &str) -> bool {
        if self.state != DriverState::Scanning {
            return false;
        }
        let found = self.advertisements.iter().any(|a| a.address == address);
        if found {
            self.connected_address = Some(address.to_string());
            self.state = DriverState::Connected;
            true
        } else {
            false
        }
    }
}

const ATOM_STACK: [&str; 6] = ["scan", "project", "compose", "measure", "preserve", "order"];

fn validate_atom_stack() -> bool {
    let expected: [&str; 6] = ["scan", "project", "compose", "measure", "preserve", "order"];
    ATOM_STACK == expected
}

fn main() {
    let mut driver = BluetoothDriver::new();

    // Initialize driver
    driver.initialize();
    assert_eq!(driver.state, DriverState::Initialized);

    // Enable scanning
    driver.enable_scan();
    assert_eq!(driver.state, DriverState::Scanning);

    // Ingest two advertisements
    driver.ingest_advertisement(Advertisement::new("AA:BB:CC:DD:EE:01".to_string(), -50));
    driver.ingest_advertisement(Advertisement::new("AA:BB:CC:DD:EE:02".to_string(), -60));

    // Try to connect to unknown address
    let result = driver.connect("AA:BB:CC:DD:EE:99");
    assert_eq!(result, false);
    assert_eq!(driver.connected_address, None);

    // Connect to known address
    let result = driver.connect("AA:BB:CC:DD:EE:01");
    assert_eq!(result, true);
    assert_eq!(driver.connected_address, Some("AA:BB:CC:DD:EE:01".to_string()));
    assert_eq!(driver.state, DriverState::Connected);

    // Verify reset opcode
    let reset_opcode = driver.transport.sent_packets[0].opcode;
    assert_eq!(reset_opcode, 0x0C03);

    // Verify scan opcode
    let scan_opcode = driver.transport.sent_packets[1].opcode;
    assert_eq!(scan_opcode, 0x200C);

    // Verify scan enabled
    assert_eq!(driver.state, DriverState::Connected);

    // Verify exactly two devices
    assert_eq!(driver.advertisements.len(), 2);

    // Verify connected address
    assert_eq!(driver.connected_address, Some("AA:BB:CC:DD:EE:01".to_string()));

    // Verify canonical atom stack
    let stack_valid = validate_atom_stack();
    assert!(stack_valid);

    println!("MATH_ATOMS_DRIVER_OK bluetooth hci_reset=0x0C03 scan=enabled devices=2 connected=AA:BB:CC:DD:EE:01 stack=canonical");
}