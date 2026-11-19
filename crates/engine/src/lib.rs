//! Pina Live's engine: decks that play tracks as a DJ does (keylock, sync, cues, loops, slip,
//! scratching), the mixer they play through, and the audio thread they run on. Decoding, song
//! study and the master's limiter come from Pina Studio's engine.

pub mod analyze;
pub mod audio;
pub mod deck;
pub mod dsp;
pub mod engine;
pub mod fx;
pub mod grid;
pub mod library;
pub mod mixer;
pub mod output;
pub mod record;
pub mod stems;

#[cfg(test)]
mod tests;
