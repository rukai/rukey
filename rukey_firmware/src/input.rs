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

            for (i, mapping) in current_profile.mappings.iter().enumerate() {
                let all_pressed = input_state.is_all_pressed(&mapping.input_set);
                state.mapping_states[i] = match state.mapping_states[i]
                    .process(mapping, all_pressed, &mut state)
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
            RukeyInput::Row0Col0 => self.pressed[0][0],
            RukeyInput::Row0Col1 => self.pressed[0][1],
            RukeyInput::Row0Col2 => self.pressed[0][2],
            RukeyInput::Row0Col3 => self.pressed[0][3],
            RukeyInput::Row0Col4 => self.pressed[0][4],
            RukeyInput::Row0Col5 => self.pressed[0][5],
            RukeyInput::Row0Col6 => self.pressed[0][6],
            RukeyInput::Row0Col7 => self.pressed[0][7],
            RukeyInput::Row0Col8 => self.pressed[0][8],
            RukeyInput::Row0Col9 => self.pressed[0][9],
            RukeyInput::Row0Col10 => self.pressed[0][10],
            RukeyInput::Row0Col11 => self.pressed[0][11],
            RukeyInput::Row0Col12 => self.pressed[0][12],
            RukeyInput::Row0Col13 => self.pressed[0][13],
            RukeyInput::Row1Col0 => self.pressed[1][0],
            RukeyInput::Row1Col1 => self.pressed[1][1],
            RukeyInput::Row1Col2 => self.pressed[1][2],
            RukeyInput::Row1Col3 => self.pressed[1][3],
            RukeyInput::Row1Col4 => self.pressed[1][4],
            RukeyInput::Row1Col5 => self.pressed[1][5],
            RukeyInput::Row1Col6 => self.pressed[1][6],
            RukeyInput::Row1Col7 => self.pressed[1][7],
            RukeyInput::Row1Col8 => self.pressed[1][8],
            RukeyInput::Row1Col9 => self.pressed[1][9],
            RukeyInput::Row1Col10 => self.pressed[1][10],
            RukeyInput::Row1Col11 => self.pressed[1][11],
            RukeyInput::Row1Col12 => self.pressed[1][12],
            RukeyInput::Row1Col13 => self.pressed[1][13],
            RukeyInput::Row2Col0 => self.pressed[2][0],
            RukeyInput::Row2Col1 => self.pressed[2][1],
            RukeyInput::Row2Col2 => self.pressed[2][2],
            RukeyInput::Row2Col3 => self.pressed[2][3],
            RukeyInput::Row2Col4 => self.pressed[2][4],
            RukeyInput::Row2Col5 => self.pressed[2][5],
            RukeyInput::Row2Col6 => self.pressed[2][6],
            RukeyInput::Row2Col7 => self.pressed[2][7],
            RukeyInput::Row2Col8 => self.pressed[2][8],
            RukeyInput::Row2Col9 => self.pressed[2][9],
            RukeyInput::Row2Col10 => self.pressed[2][10],
            RukeyInput::Row2Col11 => self.pressed[2][11],
            RukeyInput::Row2Col12 => self.pressed[2][12],
            RukeyInput::Row2Col13 => self.pressed[2][13],
            RukeyInput::Row3Col0 => self.pressed[3][0],
            RukeyInput::Row3Col1 => self.pressed[3][1],
            RukeyInput::Row3Col2 => self.pressed[3][2],
            RukeyInput::Row3Col3 => self.pressed[3][3],
            RukeyInput::Row3Col4 => self.pressed[3][4],
            RukeyInput::Row3Col5 => self.pressed[3][5],
            RukeyInput::Row3Col8 => self.pressed[3][8],
            RukeyInput::Row3Col9 => self.pressed[3][9],
            RukeyInput::Row3Col10 => self.pressed[3][10],
            RukeyInput::Row3Col11 => self.pressed[3][11],
            RukeyInput::Row3Col12 => self.pressed[3][12],
            RukeyInput::Row3Col13 => self.pressed[3][13],
            RukeyInput::Row4Col0 => self.pressed[4][0],
            RukeyInput::Row4Col1 => self.pressed[4][1],
            RukeyInput::Row4Col2 => self.pressed[4][2],
            RukeyInput::Row4Col3 => self.pressed[4][3],
            RukeyInput::Row4Col4 => self.pressed[4][4],
            RukeyInput::Row4Col9 => self.pressed[4][9],
            RukeyInput::Row4Col10 => self.pressed[4][10],
            RukeyInput::Row4Col11 => self.pressed[4][11],
            RukeyInput::Row4Col12 => self.pressed[4][12],
            RukeyInput::Row4Col13 => self.pressed[4][13],
            RukeyInput::Row5Col0 => self.pressed[5][6],
            RukeyInput::Row5Col1 => self.pressed[5][5],
            RukeyInput::Row5Col2 => self.pressed[5][4],
            RukeyInput::Row5Col3 => self.pressed[5][3],
            RukeyInput::Row5Col4 => self.pressed[5][2],
            RukeyInput::Row5Col9 => self.pressed[5][7],
            RukeyInput::Row5Col10 => self.pressed[5][8],
            RukeyInput::Row5Col11 => self.pressed[5][9],
            RukeyInput::Row5Col12 => self.pressed[5][10],
            RukeyInput::Row5Col13 => self.pressed[5][11],
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
