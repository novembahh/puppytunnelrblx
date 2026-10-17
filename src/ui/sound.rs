use std::io::Cursor;
use std::sync::mpsc::{channel, Sender};
use std::sync::OnceLock;
use std::thread;

static BUTTON_SOUND: &[u8] = include_bytes!("../../assets/sounds/button.mp3");
static SOUND_TX: OnceLock<Sender<SoundMsg>> = OnceLock::new();

enum SoundMsg {
    PlayClick,
}

fn get_sender() -> &'static Sender<SoundMsg> {
    SOUND_TX.get_or_init(|| {
        let (tx, rx) = channel::<SoundMsg>();
        if let Err(e) = thread::Builder::new()
            .name("ui-audio".to_string())
            .spawn(move || {
                run_audio_worker(rx);
            })
        {
            eprintln!("UI Audio: Failed to spawn audio thread: {}", e);
        }
        tx
    })
}

fn run_audio_worker(rx: std::sync::mpsc::Receiver<SoundMsg>) {
    let sample_buffer = match rodio::Decoder::new(Cursor::new(BUTTON_SOUND)) {
        Ok(decoder) => {
            use rodio::Source;
            let channels = decoder.channels();
            let sample_rate = decoder.sample_rate();
            let samples: Vec<_> = decoder.collect();
            Some(rodio::buffer::SamplesBuffer::new(channels, sample_rate, samples))
        }
        Err(e) => {
            eprintln!("UI Audio: Failed to decode button sound: {}", e);
            None
        }
    };

    let mut sink: Option<rodio::MixerDeviceSink> = None;

    while let Ok(msg) = rx.recv() {
        match msg {
            SoundMsg::PlayClick => {
                if let Some(ref buffer) = sample_buffer {
                    if sink.is_none() {
                        sink = rodio::DeviceSinkBuilder::open_default_sink().ok();
                    }
                    if let Some(ref s) = sink {
                        s.mixer().add(buffer.clone());
                    }
                }
            }
        }
    }
}


pub fn play_click() {
    let _ = get_sender().send(SoundMsg::PlayClick);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedded_sound_decodes() {
        assert!(!BUTTON_SOUND.is_empty(), "Embedded sound must not be empty");
        let cursor = Cursor::new(BUTTON_SOUND);
        let decoder = rodio::Decoder::new(cursor).expect("Embedded button sound must decode successfully");
        use rodio::Source;
        assert!(decoder.sample_rate().get() > 0);
        let samples: Vec<_> = decoder.collect();
        assert!(!samples.is_empty(), "Decoded samples should not be empty");
    }
}
