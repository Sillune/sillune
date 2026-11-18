use anyhow::{Context, Result, bail};
use cpal::{
    Device, FromSample, Sample, SizedSample, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

#[derive(Default)]
pub struct Microphone(Mutex<Option<Capture>>);

struct Capture {
    stream: cpal::Stream,
    samples: Arc<Mutex<Vec<f32>>>,
    rate: usize,
}

impl Microphone {
    pub fn start(&self) -> Result<()> {
        let mut current = self.0.lock().unwrap();
        if current.is_some() {
            bail!("Le microphone est déjà actif");
        }
        let device = cpal::default_host()
            .default_input_device()
            .context("Aucun microphone disponible")?;
        let config = device.default_input_config()?;
        let samples = Arc::new(Mutex::new(Vec::new()));
        let capture = match config.sample_format() {
            cpal::SampleFormat::F32 => input::<f32>(&device, &config.config(), &samples)?,
            cpal::SampleFormat::I16 => input::<i16>(&device, &config.config(), &samples)?,
            cpal::SampleFormat::U16 => input::<u16>(&device, &config.config(), &samples)?,
            _ => bail!("Format de microphone non pris en charge"),
        };
        capture.play()?;
        *current = Some(Capture {
            stream: capture,
            samples,
            rate: config.sample_rate().0 as usize,
        });
        Ok(())
    }

    pub fn stop(&self) -> Result<Vec<u8>> {
        let capture = self
            .0
            .lock()
            .unwrap()
            .take()
            .context("Le microphone est inactif")?;
        drop(capture.stream);
        let samples = capture.samples.lock().unwrap();
        if samples.len() < capture.rate / 8 {
            bail!("Enregistrement trop court");
        }
        Ok(pcm16(&samples, capture.rate))
    }
}

fn input<T: Sample + SizedSample + Send + 'static>(
    device: &Device,
    config: &StreamConfig,
    samples: &Arc<Mutex<Vec<f32>>>,
) -> Result<cpal::Stream>
where
    f32: FromSample<T>,
{
    let samples = samples.clone();
    let channels = config.channels as usize;
    let limit = config.sample_rate.0 as usize * 40;
    Ok(device.build_input_stream(
        config,
        move |data: &[T], _| {
            if let Ok(mut samples) = samples.lock() {
                let remaining = limit.saturating_sub(samples.len());
                samples.extend(data.chunks_exact(channels).take(remaining).map(|frame| {
                    frame.iter().map(|v| f32::from_sample(*v)).sum::<f32>() / channels as f32
                }));
            }
        },
        |_| {},
        None,
    )?)
}

fn pcm16(samples: &[f32], rate: usize) -> Vec<u8> {
    (0..samples.len() * 16_000 / rate)
        .flat_map(|i| {
            let position = i as f64 * rate as f64 / 16_000.0;
            let index = position as usize;
            let fraction = position.fract() as f32;
            let left = samples.get(index).copied().unwrap_or_default();
            let right = samples.get(index + 1).copied().unwrap_or(left);
            let value = (left * (1.0 - fraction) + right * fraction).clamp(-1.0, 1.0);
            ((value * i16::MAX as f32) as i16).to_le_bytes()
        })
        .collect()
}

pub fn read_wav(path: &Path) -> Result<Vec<u8>> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let channels = spec.channels as usize;
    let rate = spec.sample_rate as usize;
    if channels == 0 || channels > 32 || !(8_000..=192_000).contains(&rate) {
        bail!("Fréquence ou nombre de canaux WAV invalide");
    }
    let limit = channels * rate * 40;
    let samples = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .take(limit)
            .collect::<Result<Vec<_>, _>>()?,
        hound::SampleFormat::Int => {
            let scale = (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .take(limit)
                .map(|v| v.map(|v| v as f32 / scale))
                .collect::<Result<Vec<_>, _>>()?
        }
    };
    let mono = samples
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect::<Vec<_>>();
    if mono.is_empty() {
        bail!("Fichier WAV vide");
    }
    Ok(pcm16(&mono, rate))
}
