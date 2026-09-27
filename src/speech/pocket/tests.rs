use super::*;

#[test]
fn rejects_cloud_options_and_unknown_voices_before_loading() {
    PocketSpeech::validate(&SpeechRequest::pocket("Hello")).unwrap();
    assert!(PocketSpeech::validate(&SpeechRequest::new("Hello")).is_err());
    for voice in POCKET_VOICES {
        let mut request = SpeechRequest::pocket("Hello");
        request.voice = voice;
        PocketSpeech::validate(&request).unwrap();
    }
    let mut request = SpeechRequest::pocket("Hello");
    request.speed = f32::NAN;
    assert!(PocketSpeech::validate(&request).is_err());
    request.speed = 0.9;
    assert!(PocketSpeech::validate(&request).is_err());
    request.speed = 1.0;
    request.instructions = Some("warmly");
    assert!(PocketSpeech::validate(&request).is_err());
    assert!(PocketSpeech::validate(&SpeechRequest::pocket(" ")).is_err());
    assert!(PocketSpeech::validate(&SpeechRequest::pocket(&"x".repeat(4097))).is_err());
}

#[test]
fn pre_cancelled_synthesis_emits_terminal_event_without_assets() {
    let directory = tempfile::tempdir().unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let captured = events.clone();
    let engine = PocketSpeech::with_cache(directory.path())
        .unwrap()
        .progress(move |e| captured.lock().unwrap().push(e));
    let token = Cancellation::default();
    token.cancel();
    let result = engine.synthesize(
        &SpeechRequest::pocket("Hello"),
        &mut |_: &[i16]| panic!("cancelled"),
        &token,
    );
    assert!(matches!(result, Err(SpeechError::Cancelled)));
    assert!(matches!(
        events.lock().unwrap().as_slice(),
        [SpeechEvent::Started, SpeechEvent::Cancelled]
    ));
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn clips_pcm_and_rejects_non_finite_audio() {
    assert_eq!(
        pcm16(&[-2., -1., -0.5, 0., 0.5, 1., 2.]).unwrap(),
        [-32768, -32768, -16384, 0, 16384, 32767, 32767]
    );
    assert!(pcm16(&[f32::NAN]).is_err());
    assert!(pcm16(&[f32::INFINITY]).is_err());
}

#[test]
fn validates_voice_cache_layout_before_tensor_allocation() {
    use safetensors::{Dtype, tensor::TensorView};
    let offset = 1i64.to_le_bytes();
    let data = vec![0; 2 * 16 * 64 * 4];
    let make = |offset: &[u8], shape: Vec<usize>| {
        let tensors = [
            (
                "transformer.layers.0.self_attn/offset",
                TensorView::new(Dtype::I64, vec![1], offset).unwrap(),
            ),
            (
                "transformer.layers.0.self_attn/cache",
                TensorView::new(Dtype::F32, shape, &data).unwrap(),
            ),
        ];
        safetensors::serialize(tensors, None).unwrap()
    };
    let bytes = make(&offset, vec![2, 1, 1, 16, 64]);
    assert_eq!(
        voice_position(&safetensors::SafeTensors::deserialize(&bytes).unwrap(), 0).unwrap(),
        1
    );
    let bytes = make(&(-1i64).to_le_bytes(), vec![2, 1, 1, 16, 64]);
    assert!(voice_position(&safetensors::SafeTensors::deserialize(&bytes).unwrap(), 0).is_err());
    let bytes = make(&offset, vec![2, 1, 1, 32, 32]);
    assert!(voice_position(&safetensors::SafeTensors::deserialize(&bytes).unwrap(), 0).is_err());
}

#[test]
#[ignore = "requires cached Pocket weights; run explicitly after downloading"]
fn native_streaming_cancellation_and_reuse() {
    let engine = PocketSpeech::new().unwrap();
    let token = Cancellation::default();
    let mut chunks = 0;
    let result = engine.synthesize(
        &SpeechRequest::pocket("This is a native streaming cancellation test."),
        &mut |samples: &[i16]| {
            assert!(!samples.is_empty());
            chunks += 1;
            token.cancel();
            Ok(())
        },
        &token,
    );
    assert!(matches!(result, Err(SpeechError::Cancelled)));
    assert_eq!(chunks, 1);
    let mut samples = 0;
    let summary = engine
        .synthesize(
            &SpeechRequest::pocket("Hello from Hear."),
            &mut |chunk: &[i16]| {
                samples += chunk.len();
                Ok(())
            },
            &Cancellation::default(),
        )
        .unwrap();
    assert_eq!(summary.samples, samples as u64);
    assert!(samples > 24000);
    let result = engine.synthesize(
        &SpeechRequest::pocket("Output failure test."),
        &mut |_: &[i16]| Err(SpeechError::Output(anyhow::anyhow!("closed output"))),
        &Cancellation::default(),
    );
    assert!(matches!(result, Err(SpeechError::Output(_))));
}
