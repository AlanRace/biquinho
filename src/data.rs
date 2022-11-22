use std::{ffi::OsStr, path::PathBuf};

use bevy::{
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    sprite::Anchor,
};
use bevy_prototype_lyon::prelude::{
    DrawMode, FillMode, GeometryBuilder, PathBuilder, StrokeMode, StrokeOptions,
};
use image::GrayImage;
use imageproc::contours::find_contours;
use rand::Rng;
use std::fs::File;
use tiff::decoder::Decoder;

use crate::{image_plugin::Opacity, imc::IMCEvent, ui::UiEntry};

pub struct DataPlugin;

impl Plugin for DataPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<DataCommand>()
            .add_system(issue_data_commands);
    }
}

#[derive(Clone)]
pub enum DataCommand {
    OpenData(PathBuf),
    CloseData(Entity),
    IMCEvent(IMCEvent),
    LoadCellData(Entity, PathBuf),
}

fn issue_data_commands(
    mut commands: Commands,

    mut data_events: EventReader<DataCommand>,
    mut imc_events: EventWriter<IMCEvent>,
    mut textures: ResMut<Assets<Image>>,
) {
    for event in data_events.iter() {
        match event {
            DataCommand::OpenData(filename) => {
                // TODO: Detect file type and initiate the correct event

                imc_events.send(IMCEvent::Load(filename.clone()));
            }
            DataCommand::CloseData(entity) => {
                commands.entity(*entity).despawn_recursive();
            }
            DataCommand::IMCEvent(event) => {
                imc_events.send(event.clone());
            }
            DataCommand::LoadCellData(entity, cell_data) => {
                // let thread_pool = AsyncComputeTaskPool::get();

                // let load_task = thread_pool.spawn(async move {
                //     let file = BufReader::new(File::open(&path)?);

                //     MCD::parse_with_dcm(file, path.to_str().unwrap())
                // });
                let img_file = File::open(cell_data).expect("Cannot find cell segmentation image!");
                let mut decoder = Decoder::new(img_file).expect("Cannot create decoder");

                let (width, height) = decoder.dimensions().unwrap();

                let mut data = vec![0; (width * height) as usize];
                let image = decoder.read_image().unwrap();

                let mut max_cell_index = 0;

                match image {
                    tiff::decoder::DecodingResult::U16(cell_data) => {
                        for y in 0..height {
                            for x in 0..width {
                                let index = (y * width) + x;
                                // let data_index = index as usize * 4;

                                if cell_data[index as usize] > 0 {
                                    data[index as usize] = 255;
                                    // data[data_index + 1] = 155;
                                    // data[data_index + 2] = 255;
                                    // data[data_index + 3] = 255;
                                }

                                if cell_data[index as usize] > max_cell_index {
                                    max_cell_index = cell_data[index as usize];
                                }
                            }
                        }
                    }
                    _ => todo!(),
                }

                let grey_image = GrayImage::from_raw(width, height, data.clone()).unwrap();
                let contours = find_contours::<u32>(&grey_image);

                println!("{:?}", contours[0]);

                let mut rng = rand::thread_rng();

                for contour in contours.iter() {
                    let mut builder = PathBuilder::new();

                    let first_point = contour.points[0];
                    builder.move_to(Vec2::new(
                        first_point.x as f32 + 0.5,
                        (height - first_point.y) as f32 - 0.5,
                    ));

                    for point in contour.points.iter().skip(1) {
                        builder.line_to(Vec2::new(
                            point.x as f32 + 0.5,
                            (height - point.y) as f32 - 0.5,
                        ));
                    }

                    builder.close();

                    let path = builder.build();

                    let colour = Color::Rgba {
                        red: rng.gen_range(0.0..1.0),
                        green: rng.gen_range(0.0..1.0),
                        blue: rng.gen_range(0.0..1.0),
                        alpha: 0.75,
                    };

                    let cell_segmentation = commands
                        .spawn(GeometryBuilder::build_as(
                            &path,
                            DrawMode::Outlined {
                                fill_mode: FillMode::color(colour),
                                outline_mode: StrokeMode {
                                    options: StrokeOptions::default().with_line_width(0.1),
                                    color: colour, //Color::BLACK,
                                },
                            },
                            Transform::from_xyz(width as f32 * -0.5, height as f32 * -0.5, 10.0),
                        ))
                        .id();

                    // let cell_segmentation = commands
                    //     .spawn(SpriteBundle {
                    //         transform: Transform::from_xyz(0.0, 0.0, 1.0),
                    //         texture: textures.add(image),
                    //         sprite: Sprite {
                    //             custom_size: Some(Vec2::new(width as f32, height as f32)),
                    //             color: Color::Rgba {
                    //                 red: 1.0,
                    //                 green: 1.0,
                    //                 blue: 1.0,
                    //                 alpha: 0.5,
                    //             },
                    //             anchor: Anchor::Center,
                    //             ..Default::default()
                    //         },
                    //         ..Default::default()
                    //     })
                    //     .insert(UiEntry {
                    //         description: format!(
                    //             "Cell segmentation: {:?}",
                    //             cell_data.file_name().unwrap_or_else(|| OsStr::new(""))
                    //         ),
                    //     })
                    //     .insert(Opacity(1.0))
                    //     .insert(CellSegmentation {
                    //         num_cells: max_cell_index,
                    //     })
                    //     .id();

                    commands.entity(*entity).add_child(cell_segmentation);
                }

                let image = Image::new(
                    Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    data,
                    TextureFormat::R8Unorm,
                );

                let cell_segmentation = commands
                    .spawn(SpriteBundle {
                        transform: Transform::from_xyz(0.0, 0.0, 1.0),
                        texture: textures.add(image),
                        sprite: Sprite {
                            custom_size: Some(Vec2::new(width as f32, height as f32)),
                            color: Color::Rgba {
                                red: 1.0,
                                green: 1.0,
                                blue: 1.0,
                                alpha: 0.5,
                            },
                            anchor: Anchor::Center,
                            ..Default::default()
                        },
                        ..Default::default()
                    })
                    .insert(UiEntry {
                        description: format!(
                            "Cell segmentation: {:?}",
                            cell_data.file_name().unwrap_or_else(|| OsStr::new(""))
                        ),
                    })
                    .insert(Opacity(1.0))
                    .insert(CellSegmentation {
                        num_cells: max_cell_index,
                    })
                    .id();

                commands.entity(*entity).add_child(cell_segmentation);
            }
        }
    }
}

#[derive(Component)]
pub struct CellSegmentation {
    pub num_cells: u16,
}
