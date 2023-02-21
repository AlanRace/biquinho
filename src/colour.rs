use bevy::prelude::*;
use bevy_egui::egui::Color32;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Colour {
    Bevy(Color),
    Egui(Color32),
}

impl Colour {
    pub fn bevy(&self) -> Color {
        match self {
            Colour::Bevy(colour) => *colour,
            Colour::Egui(colour) => Color::rgba_u8(colour.r(), colour.g(), colour.b(), colour.a()),
        }
    }

    pub fn egui(&self) -> Color32 {
        match self {
            Colour::Bevy(colour) => Color32::from_rgba_premultiplied(
                (colour.r() * 255.0) as u8,
                (colour.g() * 255.0) as u8,
                (colour.b() * 255.0) as u8,
                (colour.a() * 255.0) as u8,
            ),
            Colour::Egui(colour) => *colour,
        }
    }
}

impl From<Color32> for Colour {
    fn from(value: Color32) -> Self {
        Colour::Egui(value)
    }
}

impl From<&Color32> for Colour {
    fn from(value: &Color32) -> Self {
        Colour::Egui(*value)
    }
}

impl From<Color> for Colour {
    fn from(value: Color) -> Self {
        Colour::Bevy(value)
    }
}

impl From<&Color> for Colour {
    fn from(value: &Color) -> Self {
        Colour::Bevy(*value)
    }
}
