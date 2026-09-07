pub(crate) struct MonoBuffer {
    channels: usize,
    samples: Vec<f32>,
    frame_sum: f32,
    frame_samples: usize,
}

impl MonoBuffer {
    pub(crate) fn new(channels: usize) -> Self {
        Self {
            channels,
            samples: Vec::new(),
            frame_sum: 0.0,
            frame_samples: 0,
        }
    }

    pub(crate) fn extend(&mut self, samples: impl IntoIterator<Item = f32>) {
        for sample in samples {
            self.frame_sum += sample;
            self.frame_samples += 1;
            if self.frame_samples == self.channels {
                self.samples.push(self.frame_sum / self.channels as f32);
                self.frame_sum = 0.0;
                self.frame_samples = 0;
            }
        }
    }

    pub(crate) fn finish(self) -> Vec<f32> {
        self.samples
    }
}

pub(crate) fn resample(samples: &[f32], source_rate: u32, target_rate: u32) -> Vec<f32> {
    if source_rate == target_rate {
        return samples.to_vec();
    }
    if samples.is_empty() {
        return Vec::new();
    }

    let output_len = samples.len() * target_rate as usize / source_rate as usize;
    let ratio = source_rate as f64 / target_rate as f64;
    if source_rate > target_rate {
        resample_down(samples, output_len, ratio)
    } else {
        resample_up(samples, output_len, ratio)
    }
}

fn resample_down(samples: &[f32], output_len: usize, ratio: f64) -> Vec<f32> {
    (0..output_len)
        .map(|index| {
            let center = (index as f64 + 0.5) * ratio;
            let start = (center - ratio).floor().max(0.0) as usize;
            let end = (center + ratio).ceil().min(samples.len() as f64) as usize;
            samples[start..end].iter().sum::<f32>() / (end - start) as f32
        })
        .collect()
}

fn resample_up(samples: &[f32], output_len: usize, ratio: f64) -> Vec<f32> {
    (0..output_len)
        .map(|index| {
            let position = index as f64 * ratio;
            let left = position.floor() as usize;
            let right = (left + 1).min(samples.len() - 1);
            let fraction = (position - left as f64) as f32;
            samples[left] * (1.0 - fraction) + samples[right] * fraction
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmixes_across_callback_boundaries() {
        let mut buffer = MonoBuffer::new(2);
        buffer.extend([1.0]);
        buffer.extend([-1.0, 0.5, 0.5]);
        assert_eq!(buffer.finish(), vec![0.0, 0.5]);
    }

    #[test]
    fn ignores_incomplete_final_frame() {
        let mut buffer = MonoBuffer::new(2);
        buffer.extend([0.5, 0.5, 1.0]);
        assert_eq!(buffer.finish(), vec![0.5]);
    }

    #[test]
    fn resamples_to_expected_length() {
        let samples = vec![0.0; 48_000];
        assert_eq!(resample(&samples, 48_000, 16_000).len(), 16_000);
    }

    #[test]
    fn preserves_constant_signal_when_resampling() {
        let samples = vec![0.25; 4_800];
        assert!(
            resample(&samples, 48_000, 16_000)
                .iter()
                .all(|sample| (*sample - 0.25).abs() < f32::EPSILON)
        );
        assert!(
            resample(&samples, 16_000, 48_000)
                .iter()
                .all(|sample| (*sample - 0.25).abs() < f32::EPSILON)
        );
    }

    #[test]
    fn attenuates_high_frequency_input_when_downsampling() {
        let samples = (0..4_800)
            .map(|index| if index % 2 == 0 { 1.0 } else { -1.0 })
            .collect::<Vec<_>>();
        let output = resample(&samples, 48_000, 16_000);
        let average_magnitude =
            output.iter().map(|sample| sample.abs()).sum::<f32>() / output.len() as f32;
        assert!(average_magnitude < 0.2);
    }
}
