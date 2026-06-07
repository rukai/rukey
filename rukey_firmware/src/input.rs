use crate::config::{CONFIG_UPDATED, ConfigFlash};
use crate::mapping_state::MappingState;
use core::ops::ControlFlow;
use defmt::info;
use embassy_rp::gpio::{AnyPin, Input, Output, Pin, Pull};
use embassy_rp::{Peri, PeripheralType};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embassy_time::Timer;
use heapless::Vec;
use rukey_config::{MAX_MAPPINGS, Profile, RukeyInput};
use static_cell::StaticCell;

static CURRENT_PROFILE: StaticCell<Profile> = StaticCell::new();

pub struct Inputs {
    pins: [Option<Peri<'static, AnyPin>>; 30],
    config_flash: &'static Mutex<CriticalSectionRawMutex, ConfigFlash>,
}

impl Inputs {
    pub fn new(
        config_flash: &'static Mutex<CriticalSectionRawMutex, ConfigFlash>,
        pins: [Option<Peri<'static, AnyPin>>; 30],
    ) -> Self {
        Inputs { pins, config_flash }
    }

    pub async fn process(&mut self) {
        let mut rx = CONFIG_UPDATED.receiver().unwrap();

        // lcol pins 0-6
        // lcol pins are mirrored from rcol pins, so the two innermost columns are 0 and the outermost columns are 6.
        // However, to make mapping configuration feel more natural, we convert these mirrored pins into a series of columns from 0-13 going from left to right
        // i.e. 6543210 0123456
        let col0_pin = 22; // lcol 6
        let col1_pin = 21; // lcol 5
        let col2_pin = 20; // lcol 4
        let col3_pin = 19; // lcol 3
        let col4_pin = 18; // lcol 2
        let col5_pin = 17; // lcol 1
        let col6_pin = 16; // lcol 0

        // rcol pins 0-6
        let col7_pin = 15; // rcol 0
        let col8_pin = 14; // rcol 1
        let col9_pin = 13; // rcol 2
        let col10_pin = 12; // rcol 3
        let col11_pin = 11; // rcol 4
        let col12_pin = 10; // rcol 5
        let col13_pin = 7; // rcol 6

        let row0_pin = 6;
        let row1_pin = 5;
        let row2_pin = 4;
        let row3_pin = 3;
        let row4_pin = 2;
        let row5_pin = 1;

        // Load initial config
        let _meta = self.config_flash.lock().await.load_meta().await; // TODO: pin remappings
        let current_profile = {
            // TODO: currently handles config with 0 profiles by loading the default profile which has mappings in it.
            //       this is unintuitive and should be changed.
            let profile = self.config_flash.lock().await.load_profile(0).await;
            CURRENT_PROFILE.init(profile)
        };

        let mut cols = [
            output(self.pins[col0_pin].take().unwrap()),
            output(self.pins[col1_pin].take().unwrap()),
            output(self.pins[col2_pin].take().unwrap()),
            output(self.pins[col3_pin].take().unwrap()),
            output(self.pins[col4_pin].take().unwrap()),
            output(self.pins[col5_pin].take().unwrap()),
            output(self.pins[col6_pin].take().unwrap()),
            output(self.pins[col7_pin].take().unwrap()),
            output(self.pins[col8_pin].take().unwrap()),
            output(self.pins[col9_pin].take().unwrap()),
            output(self.pins[col10_pin].take().unwrap()),
            output(self.pins[col11_pin].take().unwrap()),
            output(self.pins[col12_pin].take().unwrap()),
            output(self.pins[col13_pin].take().unwrap()),
        ];
        let rows = [
            input(self.pins[row0_pin].take().unwrap()),
            input(self.pins[row1_pin].take().unwrap()),
            input(self.pins[row2_pin].take().unwrap()),
            input(self.pins[row3_pin].take().unwrap()),
            input(self.pins[row4_pin].take().unwrap()),
            input(self.pins[row5_pin].take().unwrap()),
        ];

        let mut state = State::new();
        let mut loaded_profile_index: u8 = 0;

        static INPUT_STATE: StaticCell<RukeyInputState> = StaticCell::new();
        let input_state = INPUT_STATE.init(RukeyInputState::new());

        'main_loop: loop {
            // Detect config updates (ReloadConfig from web configurator); reset to profile 0
            if rx.try_changed().is_some() {
                state = State::new();
                loaded_profile_index = u8::MAX; // force reload via the block below
            }

            // Load new profile from flash when current profile changed
            if state.current_profile != loaded_profile_index {
                *current_profile = self
                    .config_flash
                    .lock()
                    .await
                    .load_profile(state.current_profile)
                    .await;
                loaded_profile_index = state.current_profile;
                state.mapping_states.clear();
            }

            input_state.update(&rows, &mut cols).await;

            // Restore mapping state to full length in case it was cleared earlier
            while current_profile.mappings.len() > state.mapping_states.len() {
                if state.mapping_states.push(MappingState::new()).is_err() {
                    defmt::panic!("mapping state overflow");
                }
            }

            let mut all_pressed_arr: [bool; MAX_MAPPINGS] = [false; MAX_MAPPINGS];
            for (mapping, e) in current_profile
                .mappings
                .iter()
                .zip(all_pressed_arr.iter_mut())
            {
                *e = input_state.is_all_pressed(&mapping.input_set);
            }

            for (i, mapping) in current_profile.mappings.iter().enumerate() {
                let suppressed = all_pressed_arr[i]
                    && current_profile
                        .mappings
                        .iter()
                        .enumerate()
                        .any(|(j, other)| {
                            j != i
                                && all_pressed_arr[j]
                                && other.input_set.len() > mapping.input_set.len()
                                && mapping
                                    .input_set
                                    .iter()
                                    .all(|k| other.input_set.contains(k))
                        });
                state.mapping_states[i] = match state.mapping_states[i]
                    .process(mapping, all_pressed_arr[i], suppressed, &mut state)
                    .await
                {
                    ControlFlow::Continue(ms) => ms,
                    ControlFlow::Break(()) => continue 'main_loop,
                };
            }

            Timer::after_millis(1).await;
        }
    }
}

pub(crate) struct State {
    /// The index of the currently selected profile
    pub(crate) current_profile: u8,
    /// Tracks press/release state for each mapping in the current profile
    pub(crate) mapping_states: Vec<MappingState, MAX_MAPPINGS>,
}

impl State {
    fn new() -> Self {
        State {
            current_profile: 0,
            mapping_states: Vec::new(),
        }
    }
}

struct RukeyInputState {
    // indexed [row][col]
    pub pressed: [[bool; 14]; 6],
}

impl RukeyInputState {
    fn new() -> Self {
        Self {
            pressed: Default::default(),
        }
    }

    async fn update(&mut self, rows: &[Input<'static>; 6], cols: &mut [Output<'static>; 14]) {
        self.pressed = Default::default();

        for (col_i, col) in cols.iter_mut().enumerate() {
            col.set_high();
            Timer::after_micros(30).await;
            for (row_i, row) in rows.iter().enumerate() {
                self.pressed[row_i][col_i] = row.is_high();
            }
            col.set_low();
        }
    }

    fn is_all_pressed(&self, check: &[RukeyInput]) -> bool {
        // Disable the mapping when the inputs are entirely empty
        // It is an obvious configuration mistake and having it constantly trigger the input would be very annoying
        if check.is_empty() {
            return false;
        }

        for input in check {
            if self.is_pressed(*input) {
                info!("{} held", input);
            } else {
                return false;
            }
        }
        true
    }

    fn is_pressed(&self, check: RukeyInput) -> bool {
        match check {
            RukeyInput::LeftRow0Col0 => self.pressed[0][0],
            RukeyInput::LeftRow0Col1 => self.pressed[0][1],
            RukeyInput::LeftRow0Col2 => self.pressed[0][2],
            RukeyInput::LeftRow0Col3 => self.pressed[0][3],
            RukeyInput::LeftRow0Col4 => self.pressed[0][4],
            RukeyInput::LeftRow0Col5 => self.pressed[0][5],
            RukeyInput::LeftRow0Col6 => self.pressed[0][6],
            RukeyInput::RightRow0Col6 => self.pressed[0][7],
            RukeyInput::RightRow0Col5 => self.pressed[0][8],
            RukeyInput::RightRow0Col4 => self.pressed[0][9],
            RukeyInput::RightRow0Col3 => self.pressed[0][10],
            RukeyInput::RightRow0Col2 => self.pressed[0][11],
            RukeyInput::RightRow0Col1 => self.pressed[0][12],
            RukeyInput::RightRow0Col0 => self.pressed[0][13],
            RukeyInput::LeftRow1Col0 => self.pressed[1][0],
            RukeyInput::LeftRow1Col1 => self.pressed[1][1],
            RukeyInput::LeftRow1Col2 => self.pressed[1][2],
            RukeyInput::LeftRow1Col3 => self.pressed[1][3],
            RukeyInput::LeftRow1Col4 => self.pressed[1][4],
            RukeyInput::LeftRow1Col5 => self.pressed[1][5],
            RukeyInput::LeftRow1Col6 => self.pressed[1][6],
            RukeyInput::RightRow1Col6 => self.pressed[1][7],
            RukeyInput::RightRow1Col5 => self.pressed[1][8],
            RukeyInput::RightRow1Col4 => self.pressed[1][9],
            RukeyInput::RightRow1Col3 => self.pressed[1][10],
            RukeyInput::RightRow1Col2 => self.pressed[1][11],
            RukeyInput::RightRow1Col1 => self.pressed[1][12],
            RukeyInput::RightRow1Col0 => self.pressed[1][13],
            RukeyInput::LeftRow2Col0 => self.pressed[2][0],
            RukeyInput::LeftRow2Col1 => self.pressed[2][1],
            RukeyInput::LeftRow2Col2 => self.pressed[2][2],
            RukeyInput::LeftRow2Col3 => self.pressed[2][3],
            RukeyInput::LeftRow2Col4 => self.pressed[2][4],
            RukeyInput::LeftRow2Col5 => self.pressed[2][5],
            RukeyInput::LeftRow2Col6 => self.pressed[2][6],
            RukeyInput::RightRow2Col6 => self.pressed[2][7],
            RukeyInput::RightRow2Col5 => self.pressed[2][8],
            RukeyInput::RightRow2Col4 => self.pressed[2][9],
            RukeyInput::RightRow2Col3 => self.pressed[2][10],
            RukeyInput::RightRow2Col2 => self.pressed[2][11],
            RukeyInput::RightRow2Col1 => self.pressed[2][12],
            RukeyInput::RightRow2Col0 => self.pressed[2][13],
            RukeyInput::LeftRow3Col0 => self.pressed[3][0],
            RukeyInput::LeftRow3Col1 => self.pressed[3][1],
            RukeyInput::LeftRow3Col2 => self.pressed[3][2],
            RukeyInput::LeftRow3Col3 => self.pressed[3][3],
            RukeyInput::LeftRow3Col4 => self.pressed[3][4],
            RukeyInput::LeftRow3Col5 => self.pressed[3][5],
            RukeyInput::RightRow3Col5 => self.pressed[3][8],
            RukeyInput::RightRow3Col4 => self.pressed[3][9],
            RukeyInput::RightRow3Col3 => self.pressed[3][10],
            RukeyInput::RightRow3Col2 => self.pressed[3][11],
            RukeyInput::RightRow3Col1 => self.pressed[3][12],
            RukeyInput::RightRow3Col0 => self.pressed[3][13],
            RukeyInput::LeftRow4Col0 => self.pressed[4][0],
            RukeyInput::LeftRow4Col1 => self.pressed[4][1],
            RukeyInput::LeftRow4Col2 => self.pressed[4][2],
            RukeyInput::LeftRow4Col3 => self.pressed[4][3],
            RukeyInput::LeftRow4Col4 => self.pressed[4][4],
            RukeyInput::RightRow4Col4 => self.pressed[4][9],
            RukeyInput::RightRow4Col3 => self.pressed[4][10],
            RukeyInput::RightRow4Col2 => self.pressed[4][11],
            RukeyInput::RightRow4Col1 => self.pressed[4][12],
            RukeyInput::RightRow4Col0 => self.pressed[4][13],
            RukeyInput::LeftThumbRow0Col0 => self.pressed[5][6],
            RukeyInput::LeftThumbRow0Col1 => self.pressed[5][2],
            RukeyInput::LeftThumbRow1Col0 => self.pressed[5][5],
            RukeyInput::LeftThumbRow1Col1 => self.pressed[5][4],
            RukeyInput::LeftThumbRow1Col2 => self.pressed[5][3],
            RukeyInput::RightThumbRow0Col0 => self.pressed[5][7],
            RukeyInput::RightThumbRow0Col1 => self.pressed[5][9],
            RukeyInput::RightThumbRow1Col0 => self.pressed[5][8],
            RukeyInput::RightThumbRow1Col1 => self.pressed[5][11],
            RukeyInput::RightThumbRow1Col2 => self.pressed[5][10],
        }
    }
}

// TODO: become Input::new
fn input<T: PeripheralType + Pin>(pin: Peri<'static, T>) -> Input<'static> {
    let mut pin = Input::new(pin, Pull::Down);
    pin.set_schmitt(true);
    pin
}

fn output<T: PeripheralType + Pin>(pin: Peri<'static, T>) -> Output<'static> {
    Output::new(pin, embassy_rp::gpio::Level::Low)
}
