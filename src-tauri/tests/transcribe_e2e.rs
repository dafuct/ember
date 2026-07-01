// Ignored end-to-end measurement — needs a real model file + a Ukrainian sample.
// Run manually:
//   EMBER_TEST_MODEL=/path/ggml-large-v3-turbo.bin \
//   EMBER_TEST_WAV=/path/ukrainian_sample.wav \
//   EMBER_TEST_KEYWORDS="привіт,зустріч,дякую" \
//   cargo test --test transcribe_e2e -- --ignored --nocapture
use ember_lib::{decode, transcribe};

#[test]
#[ignore]
fn ukrainian_forced_language_recall() {
    let model = std::env::var("EMBER_TEST_MODEL").expect("set EMBER_TEST_MODEL");
    let wav = std::env::var("EMBER_TEST_WAV").expect("set EMBER_TEST_WAV");
    let keywords = std::env::var("EMBER_TEST_KEYWORDS").unwrap_or_default();

    let samples = decode::decode_to_16k_mono(&wav).expect("decode wav");
    let tr = transcribe::Transcriber::load(&model, "large-v3-turbo").expect("load model");
    let text = tr
        .transcribe_samples(&samples, Some("uk"))
        .expect("transcribe")
        .to_lowercase();
    println!("--- transcript ---\n{text}\n------------------");

    let kws: Vec<String> = keywords
        .split(',')
        .map(|k| k.trim().to_lowercase())
        .filter(|k| !k.is_empty())
        .collect();
    if !kws.is_empty() {
        let hit = kws.iter().filter(|k| text.contains(k.as_str())).count();
        let recall = hit as f32 / kws.len() as f32;
        println!("keyword recall: {hit}/{} = {recall:.2}", kws.len());
        assert!(recall >= 0.5, "recall {recall:.2} below 0.5 threshold");
    }
}
