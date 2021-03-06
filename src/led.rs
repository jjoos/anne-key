use super::keymatrix::{to_packed_bits, KeyState};
use super::protocol::{LedOp, Message, MsgType};
use super::serial::{Serial, Transfer};
use super::serial::led_usart::LedUsart;
use bluetooth::BluetoothMode;
use core::marker::Unsize;
use embedded_hal::digital::OutputPin;
use hal::gpio::{Input, Output};
use hal::gpio::gpioc::PC15;
use keycodes::KeyIndex;
use nb;
use rtfm::Threshold;

pub enum LedMode {
    _Off,
    On,
    Flash,
}

pub struct Led<BUFFER: 'static + Unsize<[u8]>> {
    pub serial: Serial<LedUsart, BUFFER>,
    pub rx_transfer: Option<Transfer<BUFFER>>,
    pub pc15: PC15<Output>,
    pub state: bool,
}

impl<BUFFER> Led<BUFFER>
where
    BUFFER: Unsize<[u8]>,
{
    pub fn new(
        mut serial: Serial<LedUsart, BUFFER>,
        rx_buffer: &'static mut BUFFER,
        pc15: PC15<Input>,
    ) -> Led<BUFFER> {
        let rx_transfer = serial.receive(rx_buffer);
        Led {
            serial,
            rx_transfer: Some(rx_transfer),
            pc15: pc15.into_output().pull_up(),
            state: false,
        }
    }

    pub fn on(&mut self) -> nb::Result<(), !> {
        self.pc15.set_high();
        Ok(())
    }

    pub fn off(&mut self) -> nb::Result<(), !> {
        self.pc15.set_low();
        Ok(())
    }

    pub fn toggle(&mut self) -> nb::Result<(), !> {
        let result = if !self.state {
            self.theme_mode()
        } else {
            self.set_theme(0)
        };
        self.state = !self.state;
        result
    }

    // next_* cycles through themes/brightness/speed
    pub fn next_theme(&mut self) -> nb::Result<(), !> {
        self.serial
            .send(MsgType::Led, LedOp::ConfigCmd as u8, &[1, 0, 0])
    }

    pub fn next_brightness(&mut self) -> nb::Result<(), !> {
        self.serial
            .send(MsgType::Led, LedOp::ConfigCmd as u8, &[0, 0, 1])
    }

    pub fn next_animation_speed(&mut self) -> nb::Result<(), !> {
        self.serial
            .send(MsgType::Led, LedOp::ConfigCmd as u8, &[0, 1, 0])
    }

    pub fn set_theme(&mut self, theme: u8) -> nb::Result<(), !> {
        self.serial
            .send(MsgType::Led, LedOp::ThemeMode as u8, &[theme])
    }

    pub fn send_keys(&mut self, state: &KeyState) -> nb::Result<(), !> {
        let packed = to_packed_bits(state);
        self.serial
            .send(MsgType::Led, LedOp::Key as u8, &packed.bytes)
    }

    pub fn send_music(&mut self, keys: &[u8]) -> nb::Result<(), !> {
        self.serial.send(MsgType::Led, LedOp::Music as u8, keys)
    }

    pub fn get_theme_id(&mut self) -> nb::Result<(), !> {
        // responds with with [ThemeId]
        self.serial.send(MsgType::Led, LedOp::GetThemeId as u8, &[])
    }

    pub fn set_keys(&mut self, payload: &[u8]) -> nb::Result<(), !> {
        self.serial
            .send(MsgType::Led, LedOp::SetIndividualKeys as u8, payload)
    }

    pub fn theme_mode(&mut self) -> nb::Result<(), !> {
        self.serial.send(MsgType::Led, LedOp::ThemeMode as u8, &[])
    }

    pub fn bluetooth_mode(&mut self, mode: BluetoothMode) -> nb::Result<(), !> {
        let mode_color = match mode {
            BluetoothMode::Unknown => (0, 0, 0xff),
            BluetoothMode::Ble => (0, 0xff, 0),
            BluetoothMode::Legacy => (0xff, 0xff, 0),
        };

        #[cfg_attr(rustfmt, rustfmt_skip)]
        let payload = &[0xca, 0x0a,
            KeyIndex::Escape as u8, 0xff, 0xff, 0x00, LedMode::On as u8,
            KeyIndex::N1 as u8,     0xff, 0x00, 0x00, LedMode::Flash as u8,
            KeyIndex::N2 as u8,     0xff, 0x00, 0x00, LedMode::On as u8,
            KeyIndex::N3 as u8,     0xff, 0x00, 0x00, LedMode::On as u8,
            KeyIndex::N4 as u8,     0xff, 0x00, 0x00, LedMode::On as u8,
            KeyIndex::Equal as u8,  0x00, 0xff, 0x00, LedMode::On as u8,
            KeyIndex::B as u8,      0x00, 0xff, 0x00, LedMode::Flash as u8,
            KeyIndex::Minus as u8,     0x00, 0xff, 0x00, LedMode::On as u8,
            KeyIndex::N0 as u8,  mode_color.0, mode_color.1, mode_color.2, LedMode::On as u8,
            KeyIndex::A as u8,      0x00, 0xff, 0x00, LedMode::On as u8,
        ];

        self.set_keys(payload)
    }

    pub fn handle_message(&mut self, message: &Message) {
        match message.msg_type {
            MsgType::Led => {
                match LedOp::from(message.operation) {
                    LedOp::AckThemeMode => {
                        // data: [theme id]
                        //debug!("Led AckThemeMode {:?}", message.data).ok();
                    }
                    LedOp::AckConfigCmd => {
                        // data: [theme id, brightness, animation speed]
                        //debug!("Led AckConfigCmd {:?}", message.data).ok();
                    }
                    LedOp::AckSetIndividualKeys => {
                        // data: [202]
                    }
                    _ => {
                        debug!(
                            "lmsg: {:?} {} {:?}",
                            message.msg_type, message.operation, message.data
                        ).ok();
                    }
                }
            }
            _ => {
                debug!(
                    "lmsg: {:?} {} {:?}",
                    message.msg_type, message.operation, message.data
                ).ok();
            }
        }
    }

    pub fn poll(&mut self) {
        let result = self.rx_transfer
            .as_mut()
            .unwrap()
            .poll(&mut self.serial.usart);
        match result {
            Err(nb::Error::WouldBlock) => {}
            Ok(()) => {
                let buffer = self.rx_transfer.take().unwrap().finish();

                {
                    let buffer: &mut [u8] = buffer;
                    let message = Message {
                        msg_type: MsgType::from(buffer[0]),
                        operation: buffer[2],
                        data: &buffer[3..3 + buffer[1] as usize - 1],
                    };
                    self.handle_message(&message);
                }

                self.rx_transfer = Some(self.serial.receive(buffer));
            }
        }
    }
}

pub fn rx(_t: &mut Threshold, mut r: super::DMA1_CHANNEL3::Resources) {
    r.LED.poll();
}

pub fn tx(_t: &mut Threshold, mut r: super::DMA1_CHANNEL2::Resources) {
    r.LED.serial.tx_interrupt();
}
