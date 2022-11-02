//#![warn(clippy::missing_docs_in_private_items)]
#![warn(missing_docs)]
//! Biquinho - visualisation of imaging mass cytometry data.
//!
//! Functionality is split out into separate plugins. This main file deals only with the setup.
//!
//! # Features
//!
//! - [x] Load .mcd files and visualise optical data
//! - [x] Select channels to form RGB composite with user-specified thresholds
//! - [x] View multiple IMC acquisitions in single .mcd file at once (multi-camera)
//! - [ ] View multiple .mcd files
//! - [x] Add annotatations and annotate data (e.g. with pencil tool)
//! - [ ] Classify IMC data based on annotations
//! - [ ] Segment cells based on classification data
//! - [x] Load and visualise cell segmentation results
//! - [ ] Calculate per-cell statistics (e.g. channel intensity)
//!
//!
//!
//! ## TODO: View multiple .mcd files
//! - [ ] Implement means of dragging entire dataset
//! - [ ] Provide option to load new dataset above loaded datasets

use std::{fs::File, io::BufReader, path::PathBuf};

/// AnnotationPlugin - handles everything related to drawing, saving and loading annotations.
mod annotation;
/// CameraPlugin - handles viewing data view one or more cameras and selecting and dragging objects viewed in the camera.
mod camera;
mod data;
#[cfg(all(feature = "imc", feature = "msi"))]
mod data_collection;
// mod geometry;
/// ImagePlugin - handles loading and viewing image data (including channel images).
mod image_plugin;
/// IMCPlugin - handles specific loading and visualisation of imaging mass cytometry data.
mod imc;
/// Helper functions and structs for dealing with transformations (affine).
mod transform;
/// UiPlugin - handles everything related to the user interface (currently everything egui related).
mod ui;
//mod imc_ui;
#[cfg(feature = "msi")]
mod imzml;
#[cfg(feature = "msi")]
mod msi;
#[cfg(all(feature = "imc", feature = "msi"))]
mod multimodal;
#[cfg(feature = "msi")]
mod scils;
#[cfg(feature = "msi")]
mod spectrum;

use bevy::{
    diagnostic::LogDiagnosticsPlugin, prelude::*, render::texture::ImageSettings,
    tasks::AsyncComputeTaskPool, window::WindowId, winit::WinitWindows,
};
use bevy_prototype_lyon::plugin::ShapePlugin;
use data::DataPlugin;
use image_plugin::ImagePlugin;
use imc::LoadIMC;
//use imc::{ChannelImage, IMCDataset};
use camera::CameraEvent;
use imc_rs::MCD;

use transform::AffineTransform;

use crate::{imc::IMCPlugin, ui::UiPlugin};

fn main() {
    let mut app = App::new();
    let app = app
        .insert_resource(WindowDescriptor {
            title: "Biquinho".to_string(),
            //present_mode: PresentMode::Fifo,
            // scale_factor_override: Some(1.0),
            ..Default::default()
        })
        .insert_resource(ImageSettings::default_nearest())
        .insert_resource(ClearColor(Color::rgb(0.3, 0.3, 0.3)))
        .add_plugins(DefaultPlugins)
        //.insert_resource(Msaa { samples: 4 })
        .add_plugin(UiPlugin)
        .add_plugin(ImagePlugin)
        .add_plugin(DataPlugin)
        .add_plugin(LogDiagnosticsPlugin::default())
        // .add_plugin(FrameTimeDiagnosticsPlugin::default())
        .add_plugin(ShapePlugin);

    #[cfg(feature = "imc")]
    let app = app.add_plugin(IMCPlugin);

    #[cfg(feature = "msi")]
    let app = app.add_plugin(MSIPlugin);

    //.add_plugin(PencilPlugin)
    app.add_startup_system(load_test_data)
        // .add_startup_system(load_data)
        //.add_system(read_data_collection_stream)
        //.add_startup_system(load_assets.system())
        .add_startup_system(setup)
        .add_startup_system(set_window_icon)
        .add_system(print_messages)
        .run();
}

/// Set the window icon - startup system
fn set_window_icon(
    // we have to use `NonSend` here
    windows: NonSend<WinitWindows>,
) {
    let primary = windows.get_window(WindowId::primary()).unwrap();

    // here we use the `image` crate to load our icon data from a png file
    // this is not a very bevy-native solution, but it will do
    let (icon_rgba, icon_width, icon_height) = {
        let image = image::open("assets/biquihno_icon.png")
            .expect("Failed to open icon path")
            .into_rgba8();
        let (width, height) = image.dimensions();
        let rgba = image.into_raw();
        (rgba, width, height)
    };

    let icon = winit::window::Icon::from_rgba(icon_rgba, icon_width, icon_height).unwrap();

    primary.set_window_icon(Some(icon));
}

/// Severity of the error message
#[derive(Debug)]
pub enum Severity {
    /// Information - not an error
    Info,
    /// Warning - not an error
    Warning,
    /// Error - something has gone wrong
    Error,
    /// Fatal error - something has gone wrong and we can't recover
    Fatal,
}

/// Message component with message and severity.
#[derive(Component)]
pub struct Message {
    /// The severity of the message.
    pub severity: Severity,
    /// The message itself.
    pub message: String,
}

impl From<std::io::Error> for Message {
    fn from(error: std::io::Error) -> Self {
        Self {
            severity: Severity::Error,
            message: error.to_string(),
        }
    }
}

/// Load in a dataset for testing automatically - if the dataset isn't found, then just skip loading data
fn load_test_data(mut commands: Commands) {
    let thread_pool = AsyncComputeTaskPool::get();

    let path = PathBuf::from(
        // "/home/alan/Documents/Work/Nicole/Salmonella/2019-10-25_Salmonella_final_VS+WT.mcd",
        "/home/alan/Documents/Work/20200609_ARDS_1921.mcd",
    );

    let file = match File::open(&path) {
        Ok(file) => file,
        Err(_error) => {
            println!("Not opening test data.");
            return;
        }
    };

    let load_task = thread_pool.spawn(async move {
        let reader = BufReader::new(file);

        let mcd = MCD::parse_with_dcm(reader, path.to_str().unwrap())?;

        // let xml = mcd.xml()?;
        // std::fs::write("mcd.xml", xml);

        Ok(mcd)
    });
    commands.spawn().insert(LoadIMC(load_task));

    // load_imc(mcd, &mut commands, &mut textures, &thread_pool);

    #[cfg(feature = "msi")]
    {
        let mut path = PathBuf::from(
            "/home/alan/Documents/Work/Nicole/Salmonella/panchali_set-oct19_pos_s5_50um.imzML",
        );

        let start = Instant::now();
        path.set_extension("dat");
        if let Ok(header) = msi_format::parse(&path) {
            let msi_chunked: MSIChunked = header.into();
            let duration = start.elapsed();

            println!("Time elapsed parsing chunked data is: {:?}", duration);

            let msi_dataset = MSIDataset::new(Box::new(msi_chunked));

            let moving_points: Vec<Vector3<f64>> = vec![
                Vector3::new(19.99999999999996, 121.00000000000006, 0.0),
                Vector3::new(620.0812800000001, 406.0055999999998, 0.0),
                Vector3::new(524.9307875066394, 100.22192074893758, 0.0),
            ];

            // Get transform
            let fixed_points: Vec<Vector3<f64>> = vec![
                Vector3::new(1708.2446857383977, 669.5510653929076, 0.0),
                Vector3::new(4701.630281620892, 2088.2932549312454, 0.0),
                Vector3::new(4228.058794080685, 568.3346466234503, 0.0),
            ];
            println!("Fixed: {:?}", fixed_points);
            println!("Moving: {:?}", moving_points);

            let transform = AffineTransform::from_points(
                "affine_transform".to_string(),
                fixed_points,
                moving_points,
            )
            .scale(10.00000000000001, 10.00000000000001, 1.0);

            commands
                .spawn()
                .with_children(|parent| {
                    for acquisition in msi_dataset.acquisitions() {
                        // let to_parent_transform =
                        // AffineTransform::new(
                        //     format!("acquisition_to_parent").to_string(),
                        //     Matrix4::identity(),
                        // );

                        parent
                            .spawn_bundle(SpriteBundle {
                                transform: Transform::from_xyz(0.0 / 2.0, 25000.0 / 2.0, 0.0)
                                    .mul_transform(create_transform(
                                        &transform,
                                        acquisition.width() as f32,
                                        0.0, //acquisition.height() as f32,
                                        false,
                                    ))
                                    .mul_transform(Transform::from_xyz(0.0, 0.0, 20.0)),
                                sprite: Sprite {
                                    anchor: Anchor::Center,
                                    ..default()
                                },
                                ..default()
                            })
                            .insert(Draggable)
                            .insert(UiEntry {
                                description: acquisition.id().to_owned(),
                            })
                            .insert(Acquisition {
                                id: acquisition.id().to_owned(),
                            });
                    }
                    //})
                })
                .insert(PrimaryUiEntry {
                    description: format!("MSI: {:?}", path.file_name()),
                })
                .insert(msi_dataset)
                .insert(Transform::from_xyz(0.0, 0.0, 1.0))
                .insert(GlobalTransform::default());
        } else {
            println!("No MSI data present")
        }
    }
}

/// Convert an `AffineTransform` into a bevy `Transform`.
///
/// We have to modify the transform to take into account that Bevy relies on the centre of image and upsidedown
fn create_transform(
    transform: &AffineTransform,
    width: f32,
    height: f32,
    flip_required: bool,
) -> Transform {
    let translate_transform = Transform::from_xyz((width / 2.0) as f32, (height / 2.0) as f32, 0.0);
    //let scale_transform = Transform::from_scale(Vec3::new(1.0, 1.0, 0.0));
    let transform = Transform::from_matrix(transform.into());

    if flip_required {
        let transform = transform.mul_transform(translate_transform);
        println!("Trans: {:?}", transform);

        transform.with_translation(Vec3::new(
            transform.translation.x,
            25000.0 - transform.translation.y,
            0.0,
        ))
        //.mul_transform(Transform::from_scale(Vec3::new(1.0, -1.0, 1.0)))
        //.mul_transform()
    } else {
        transform.mul_transform(translate_transform)
    }
    //    .mul_transform(scale_transform)
}

/// This setup function generates a grid with a spacing of 2000 um
fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut camera_events: EventWriter<CameraEvent>,
) {
    // Send an event to enable dragging of camera
    camera_events.send(CameraEvent::EnableDragging);

    // Draw grid lines
    // TODO: Move the grid generation to its own plugin?
    let grid_length = 1000000.0;
    let grid_spacing = 2000.0;
    let grid_thickness = 100.0;

    let num_gridlines = 100;

    let grid_alpha = 0.25;

    for y in 0..=num_gridlines {
        let y_value = (-(num_gridlines / 2) + y) as f32 * grid_spacing;

        commands.spawn_bundle(SpriteBundle {
            transform: Transform::from_xyz(0.0, y_value, 0.0),
            sprite: Sprite {
                custom_size: Some(Vec2::new(grid_length, grid_thickness)),
                color: Color::rgba(0.75, 0.75, 0.75, grid_alpha),
                ..Default::default()
            },
            ..Default::default()
        });

        if y_value >= 0.0 {
            commands.spawn_bundle(Text2dBundle {
                // Use `Text` directly
                text: Text {
                    // Construct a `Vec` of `TextSection`s
                    sections: vec![TextSection {
                        value: format!("{}", y_value),
                        style: TextStyle {
                            font: asset_server.load("fonts/lato/Lato-Bold.ttf"),
                            font_size: 60.0,
                            color: Color::WHITE,
                        },
                    }],
                    alignment: TextAlignment {
                        vertical: VerticalAlign::Center,
                        horizontal: HorizontalAlign::Right,
                    },
                },
                transform: Transform::from_xyz(-300.0, y_value, 1.0)
                    .mul_transform(Transform::from_scale(Vec3::new(10.0, 10.0, 1.0))),
                ..default()
            });
        }
    }

    for x in 0..=num_gridlines {
        let x_value = (-(num_gridlines / 2) + x) as f32 * grid_spacing;

        commands.spawn_bundle(SpriteBundle {
            //material: materials.add(Color::rgb(0.5, 0.5, 1.0).into()),
            transform: Transform::from_xyz(x_value, 0.0, 0.0),
            sprite: Sprite {
                custom_size: Some(Vec2::new(grid_thickness, grid_length)),
                color: Color::rgba(0.75, 0.75, 0.75, grid_alpha),
                ..Default::default()
            },
            ..Default::default()
        });

        if x_value >= 0.0 {
            commands
            .spawn_bundle(Text2dBundle {
                // Use `Text` directly
                text: Text {
                    // Construct a `Vec` of `TextSection`s
                    sections: vec![
                        TextSection {
                            value: format!("{}", x_value),
                            style: TextStyle {
                                font: asset_server.load("fonts/lato/Lato-Bold.ttf"),
                                font_size: 60.0,
                                color: Color::WHITE,
                            },
                        },
                    ],
                    alignment: TextAlignment {
                        vertical: VerticalAlign::Center,
                        horizontal: HorizontalAlign::Center,
                    },
                },
                transform: Transform::from_xyz(
                    (-(num_gridlines / 2) + x) as f32 * grid_spacing,
                    -300.0,
                    1.0,
                ).mul_transform(Transform::from_scale(Vec3::new(10.0, 10.0, 1.0))),
                ..default()
            })
            //.insert(ColorText)
            ;
        }
    }
}

/// Print any messages to the console.
fn print_messages(q_errors: Query<&Message>) {
    for error in q_errors.iter() {
        println!("{}", error.message);
    }
}
