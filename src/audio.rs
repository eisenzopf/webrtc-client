use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SizedSample};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;
use webrtc::track::track_remote::TrackRemote;
use cpal::SampleFormat;

pub struct AudioCapture {
    input_stream: cpal::Stream,
    track: Arc<TrackLocalStaticSample>,
}

impl AudioCapture {
    pub fn new(track: Arc<TrackLocalStaticSample>) -> Result<Self> {
        let host = cpal::default_host();
        let input_device = host.default_input_device()
            .ok_or_else(|| anyhow::anyhow!("No input device available"))?;

        let config = input_device.default_input_config()?;
        println!("Input config: {:?}", config);

        let input_stream = match config.sample_format() {
            SampleFormat::F32 => Self::build_input_stream::<f32>(&input_device, &config.into(), track.clone())?,
            SampleFormat::I16 => Self::build_input_stream::<i16>(&input_device, &config.into(), track.clone())?,
            SampleFormat::U16 => Self::build_input_stream::<u16>(&input_device, &config.into(), track.clone())?,
            sample_format => return Err(anyhow::anyhow!("Unsupported sample format: {:?}", sample_format)),
        };

        input_stream.play()?;

        Ok(Self {
            input_stream,
            track,
        })
    }

    fn build_input_stream<T>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        track: Arc<TrackLocalStaticSample>,
    ) -> Result<cpal::Stream>
    where
        T: SizedSample + Sample + Send + 'static,
    {
        let err_fn = |err| eprintln!("An error occurred on the input audio stream: {}", err);

        let stream = device.build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                let samples: Vec<f32> = data.iter()
                    .map(|sample| sample.to_float())
                    .collect();
                
                if let Err(e) = futures::executor::block_on(track.write_sample(&samples)) {
                    eprintln!("Failed to write audio sample: {}", e);
                }
            },
            err_fn,
            None,
        )?;

        Ok(stream)
    }
}

pub struct AudioPlayback {
    output_stream: cpal::Stream,
    sample_rx: mpsc::Receiver<Vec<f32>>,
}

impl AudioPlayback {
    pub fn new(track: Arc<TrackRemote>) -> Result<Self> {
        let host = cpal::default_host();
        let output_device = host.default_output_device()
            .ok_or_else(|| anyhow::anyhow!("No output device available"))?;

        let config = output_device.default_output_config()?;
        println!("Output config: {:?}", config);

        let (sample_tx, sample_rx) = mpsc::channel(1024);

        // Set up track data callback
        let track_clone = track.clone();
        tokio::spawn(async move {
            while let Ok(rtp) = track_clone.read_rtp().await {
                if let Ok(samples) = rtp.payload.chunks(4)
                    .map(|chunk| {
                        let value = f32::from_le_bytes([
                            chunk[0], chunk[1], chunk[2], chunk[3]
                        ]);
                        Ok(value)
                    })
                    .collect::<Result<Vec<f32>>>() {
                    let _ = sample_tx.send(samples).await;
                }
            }
        });

        let output_stream = match config.sample_format() {
            SampleFormat::F32 => Self::build_output_stream::<f32>(&output_device, &config.into(), sample_rx.clone())?,
            SampleFormat::I16 => Self::build_output_stream::<i16>(&output_device, &config.into(), sample_rx.clone())?,
            SampleFormat::U16 => Self::build_output_stream::<u16>(&output_device, &config.into(), sample_rx.clone())?,
            sample_format => return Err(anyhow::anyhow!("Unsupported sample format: {:?}", sample_format)),
        };

        output_stream.play()?;

        Ok(Self {
            output_stream,
            sample_rx,
        })
    }

    fn build_output_stream<T>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        sample_rx: mpsc::Receiver<Vec<f32>>,
    ) -> Result<cpal::Stream>
    where
        T: SizedSample + Sample + Send + 'static,
    {
        let sample_rx = Arc::new(Mutex::new(sample_rx));
        let err_fn = |err| eprintln!("An error occurred on the output audio stream: {}", err);

        let stream = device.build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                let rx = sample_rx.clone();
                if let Ok(mut rx_guard) = rx.lock() {
                    if let Ok(samples) = rx_guard.try_recv() {
                        for (output, input) in data.iter_mut().zip(samples.iter()) {
                            *output = T::from_float_value(*input);
                        }
                    } else {
                        // Output silence if no samples available
                        for sample in data.iter_mut() {
                            *sample = T::from_float_value(0.0);
                        }
                    }
                }
            },
            err_fn,
            None,
        )?;

        Ok(stream)
    }
} 