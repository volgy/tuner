use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, FromSample, Sample, SampleFormat, SizedSample, StreamConfig};
use std::collections::VecDeque;
use std::sync::mpsc::channel;

mod tracer;
use tracer::Tracer;

struct Note {
    name: &'static str,
    cents: f32,
}

impl Note {
    const NOTE_NAMES: [&'static str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];

    const A4_FREQ: f32 = 440.0;

    fn from_frequency(frequency: f32) -> Self {
        let semitones_from_a4 = 12.0 * (frequency / Self::A4_FREQ).log2();
        let midi_number = semitones_from_a4.round() as i32 + 69;
        let note_index = (midi_number % 12 + 12) % 12; // ensure positive index
        let note_name = Self::NOTE_NAMES[note_index as usize];
        let exact_freq = Self::A4_FREQ * 2f32.powf((midi_number - 69) as f32 / 12.0);
        let cents = 1200.0 * (frequency / exact_freq).log2();
        Note {
            name: note_name,
            cents,
        }
    }
}

fn high_pass(samples: &mut [f32], sample_rate: u32, cutoff: f32) {
    let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff);
    let dt = 1.0 / sample_rate as f32;
    let alpha = dt / (rc + dt);

    let mut previous = samples[0];
    for sample in samples.iter_mut() {
        let filtered = alpha * (*sample - previous) + previous;
        previous = filtered;
        *sample = filtered;
    }
}

fn yin_pitch(
    samples: &[f32],
    sample_rate: u32,
    freq_min: f32,
    freq_max: f32,
    threshold: f32,
) -> Option<f32> {
    let n = samples.len();
    let tau_min = (sample_rate as f32 / freq_max) as usize;
    let tau_max = ((sample_rate as f32 / freq_min) as usize).min(n / 2);
    let mut yin_buffer = vec![0.0; tau_max + 1];

    // Step 1: Difference function
    for tau in tau_min..=tau_max {
        for i in 0..(n - tau) {
            let delta = samples[i] - samples[i + tau];
            yin_buffer[tau] += delta * delta;
        }
    }

    // Step 2: Cumulative mean normalized difference function
    let mut running_sum = 0.0;
    for tau in tau_min..=tau_max {
        running_sum += yin_buffer[tau];
        yin_buffer[tau] *= tau as f32 / running_sum;
    }

    // Step 3: Absolute threshold
    let mut tau = tau_min;
    let mut best_tau = None;
    while tau <= tau_max {
        if yin_buffer[tau] < threshold {
            while tau < tau_max && yin_buffer[tau + 1] < yin_buffer[tau] {
                tau += 1;
            }
            best_tau = Some(tau);
            break;
        }
        tau += 1;
    }

    // Step 3b: Fallback to global minimum
    if best_tau.is_none() {
        best_tau = (tau_min..=tau_max).min_by(|&a, &b| {
            yin_buffer[a]
                .partial_cmp(&yin_buffer[b])
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    // Step 4: Parabolic interpolation
    if let Some(rough_tau) = best_tau {
        if rough_tau > 0 && rough_tau < tau_max {
            let s0 = yin_buffer[rough_tau - 1];
            let s1 = yin_buffer[rough_tau];
            let s2 = yin_buffer[rough_tau + 1];
            let interp_tau = rough_tau as f32 + (s2 - s0) / (2.0 * (2.0 * s1 - s2 - s0));
            Some(sample_rate as f32 / interp_tau)
        } else {
            Some(sample_rate as f32 / rough_tau as f32)
        }
    } else {
        None
    }
}

fn tuner(mut samples: Vec<f32>, sample_rate: u32) {
    // let mut tracer = Tracer::new("Tuner");
    // TODO: Apply windowing function here (e.g., Hanning window)

    high_pass(&mut samples, sample_rate, 20.0);
    // tracer.mark("High-pass filter applied");

    if let Some(frequency) = yin_pitch(&samples, sample_rate, 50.0, 2000.0, 0.1) {
        //let (note, cents) = freq_to_note(frequency);
        let note = Note::from_frequency(frequency);
        println!("Note: {} ({:+.2} cents)", note.name, note.cents);
    }
    // tracer.mark("Pitch detection completed");
}

fn run<T>(device: &Device, config: &StreamConfig) -> Result<(), anyhow::Error>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let n_channels = config.channels as usize;
    let sample_rate = config.sample_rate.0;
    let mut buffer = VecDeque::with_capacity(sample_rate as usize); // 1 second buffer (initially)

    // choose window ≈ sample_rate/10 but rounded to nearest power of two
    let window_len = {
        let target = (sample_rate / 10).max(1);
        let prev = 1u32 << (31 - target.leading_zeros());
        let next = prev.saturating_mul(2);
        let chosen = if target - prev <= next - target {
            prev
        } else {
            next
        };
        chosen as usize
    };
    let hop = window_len / 2;

    let (tx, rx) = channel();

    let stream = device
        .build_input_stream(
            config,
            move |data: &[T], _info| {
                let samples: Vec<_> = data
                    .chunks_exact(n_channels)
                    .map(|frame| <f32 as Sample>::from_sample(frame[0]))
                    .collect();
                tx.send(samples)
                    .expect("failed to send data through channel");
            },
            move |err| {
                eprintln!("Stream error: {}", err);
            },
            None,
        )
        .expect("failed to build input stream");

    stream.play().expect("failed to recording stream");

    println!("Listening... Press Ctrl+C to stop.");
    loop {
        let samples = rx.recv().expect("failed to receive samples from channel");
        buffer.extend(samples);
        while buffer.len() >= window_len {
            let window: Vec<_> = buffer.iter().cloned().take(window_len).collect();
            tuner(window, sample_rate);
            buffer.drain(..hop);
        }
    }
}

fn main() -> anyhow::Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .expect("no input device available");

    println!("Device: {}", device.name().unwrap_or("Unknown".to_string()));

    let default_config = device
        .default_input_config()
        .expect("error while querying default config");
    println!("Channels: {}", default_config.channels());
    println!("Sample Rate: {}", default_config.sample_rate().0);

    let config = default_config.config();

    match default_config.sample_format() {
        SampleFormat::I8 => run::<i8>(&device, &config),
        SampleFormat::I16 => run::<i16>(&device, &config),
        SampleFormat::I24 => run::<i32>(&device, &config),
        SampleFormat::I32 => run::<i32>(&device, &config),
        SampleFormat::I64 => run::<i64>(&device, &config),
        SampleFormat::U8 => run::<u8>(&device, &config),
        SampleFormat::U16 => run::<u16>(&device, &config),
        SampleFormat::U32 => run::<u32>(&device, &config),
        SampleFormat::F32 => run::<f32>(&device, &config),
        SampleFormat::F64 => run::<f64>(&device, &config),
        sample_format => panic!("unsupported sample format '{sample_format}'"),
    }
}
