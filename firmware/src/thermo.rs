//! The die temperature. esp-hal has no driver for this sensor on the S3, so this reads its
//! registers the way ESP-IDF does, with the same conversion and without its factory offset.
use esp_hal::peripherals::SENS;

const READY_TRIES: u32 = 10_000;
const PER_COUNT: f32 = 0.4386;
const AT_ZERO: f32 = 20.52;

pub fn start() {
    let sensor = SENS::regs();
    sensor
        .sar_peri_clk_gate_conf()
        .modify(|_, set| set.tsens_clk_en().set_bit());
    sensor.sar_tsens_ctrl().modify(|_, set| {
        set.sar_tsens_power_up_force()
            .set_bit()
            .sar_tsens_power_up()
            .set_bit()
    });
}

/// Degrees Celsius, or `None` if the sensor did not answer.
pub fn read() -> Option<f32> {
    let sensor = SENS::regs().sar_tsens_ctrl();
    sensor.modify(|_, set| set.sar_tsens_dump_out().set_bit());
    let ready = (0..READY_TRIES).any(|_| sensor.read().sar_tsens_ready().bit_is_set());
    let raw = sensor.read().sar_tsens_out().bits();
    sensor.modify(|_, set| set.sar_tsens_dump_out().clear_bit());
    ready.then(|| PER_COUNT * f32::from(raw) - AT_ZERO)
}
