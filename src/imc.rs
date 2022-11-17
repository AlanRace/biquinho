use std::fmt::Write;
use std::io::BufWriter;
use std::ops::Deref;
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    path::PathBuf,
    sync::Arc,
    time::Instant,
};

use bevy::{
    prelude::*,
    reflect::TypeUuid,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    sprite::Anchor,
    tasks::{AsyncComputeTaskPool, Task},
};
use egui::Color32;
use futures_lite::future;

use nalgebra::Matrix4;
use tiff::encoder::{colortype, TiffEncoder};

use imc_rs::{
    error::MCDError, AcquisitionChannel, AcquisitionIdentifier, ChannelIdentifier, OnSlide, MCD,
};

use smartcore::{
    linalg::{naive::dense_matrix::DenseMatrix, BaseMatrix},
    tree::decision_tree_classifier::DecisionTreeClassifier,
};

use crate::camera::BoundingBox;
use crate::colour::Colour;
use crate::image_plugin::{
    ComputeTileImage, ImageControl, ImageUpdateType, Opacity, TiledImage, ToTileImage,
};
use crate::{
    annotation::{Annotation, PixelAnnotationConf},
    // data_collection::{DataCollection, Dataset, FullImage, ImageData, View},
    camera::Draggable,
    create_transform,
    transform::AffineTransform,
    ui::{PrimaryUiEntry, UiEntry},
};
use crate::{Message, Severity};

/// IMCPlugin
///
/// This includes all events and systems required to load and visualise imaging mass cytometry (IMC) data.
///
/// The easiest way to interact with this plugin is via an `IMCEvent`. This plugin will
/// consume all `IMCEvent`s present at each frame and issue the respective commands.
pub struct IMCPlugin;

impl Plugin for IMCPlugin {
    fn build(&self, app: &mut App) {
        app.add_asset::<ChannelImage>()
            .add_event::<IMCEvent>()
            .add_system(handle_imc_event)
            .add_system(load_imc)
            .add_system(apply_classifier)
            .add_system(process_classifier_results)
            .add_system(generate_channel_image.label("GenerateImage"))
            .add_system(generate_histogram.before("GenerateImage")) // This has to be before -> I think entities are despawned at the end of the frame. If this is set to after, then it tries to generate the wrong histogram
            .add_system(image_control_changed.after("GenerateImage"));
    }
}

/// IMC events define ways to interact with this plugin.
#[derive(Clone)]
pub enum IMCEvent {
    /// Loads the .mcd file at the specified location and adds all data to the world.
    /// The data is added in a hierarchy:
    /// - `Slide`
    /// -- `Panorama`
    /// --- `Acquisition`
    Load(PathBuf),
    /// Generates an image with the same size as the `target` (`PixelAnnotationTarget`), where each pixel
    /// is labelled with one of the given `labels`.
    GeneratePixelAnnotation {
        labels: Vec<Entity>,
        target: PixelAnnotationTarget, //Vec<Entity>,
        channels: Vec<ChannelIdentifier>,
        output: ClassifierOutput,
    },

    SetBackgroundOpacity {
        entity: Entity,
        opacity: f32,
    },

    SetHistogramScale {
        entity: Entity,
        scale: HistogramScale,
    },
}

/// Handle all `IMCEvent`s
fn handle_imc_event(
    mut commands: Commands,
    mut events: EventReader<IMCEvent>,
    q_acquisitions: Query<(Entity, &Acquisition, &GlobalTransform)>,
    mut q_imc: Query<&mut IMCDataset>,
    q_annotations: Query<(Entity, &Annotation)>,
) {
    let thread_pool = AsyncComputeTaskPool::get();

    for event in events.iter() {
        match event {
            IMCEvent::Load(location) => {
                let path = location.clone();

                let load_task = thread_pool.spawn(async move { MCD::from_path(path)?.with_dcm() });

                commands.spawn(LoadIMC(load_task));

                //load_imc(mcd, &mut commands, &mut textures, &thread_pool);
            }
            IMCEvent::SetBackgroundOpacity { entity, opacity } => {
                if let Ok(mut imc) = q_imc.get_mut(*entity) {
                    imc.background_alpha = *opacity;
                }
            }
            IMCEvent::SetHistogramScale { entity, scale } => {
                if let Ok(mut imc) = q_imc.get_mut(*entity) {
                    imc.histogram_scale = *scale;
                }
            }
            IMCEvent::GeneratePixelAnnotation {
                labels,
                target,
                channels,
                output,
            } => {
                if channels.is_empty() {
                    // Nothing to learn in this case, so just skip this event
                    continue;
                }

                let start = Instant::now();

                // Get the labels/annnotations in the correct form.
                // Here we convert the `Entity`s we have to `&Annotation`s
                let labels = labels
                    .iter()
                    .map(|entity| {
                        let (_, annotation) = q_annotations.get(*entity).unwrap();

                        annotation.clone()
                    })
                    .collect::<Vec<_>>();

                // Here we convert the `&crate::imc::Acquisition`s to `imc_rs::Acquisitions`, which are needed to access data.
                let acquisitions = q_acquisitions
                    .iter()
                    .map(|(_entity, acquisition, transform)| (acquisition.clone(), *transform))
                    .collect::<Vec<_>>();

                let channels_copy = channels.to_vec();
                let target_copy = target.clone();
                let output = output.clone();

                println!(
                    "Time to create copies {:?}",
                    Instant::now().duration_since(start)
                );

                let load_task = thread_pool.spawn(async move {
                    let start = Instant::now();

                    let (classification_data, classification_labels, labels) =
                        create_labelled_data(labels, acquisitions, &channels_copy);

                    println!(
                        "Time to create labelled data {:?}",
                        Instant::now().duration_since(start)
                    );

                    let start = Instant::now();
                    let x = DenseMatrix::from_2d_vec(&classification_data);
                    println!("{:?}", x.shape());
                    let tree =
                        DecisionTreeClassifier::fit(&x, &classification_labels, Default::default())
                            .unwrap();

                    println!(
                        "Time to create decision tree {:?}",
                        Instant::now().duration_since(start)
                    );

                    Classifier {
                        target: target_copy,
                        channels: channels_copy,
                        tree: Arc::new(tree),
                        labels,
                        output,
                    }
                });

                commands.spawn(BuildClassifier(load_task));

                // let (classification_data, classification_labels, label_colours) =
                //     create_labelled_data(labels, acquisitions, channels);
            }
        }
    }
}

#[derive(Clone)]
pub enum PixelAnnotationTarget {
    Region(imc_rs::BoundingBox<f64>),
    Acquisitions(Vec<String>),
}

#[derive(Component)]
pub struct GenerateChannelImage {
    pub identifier: Option<ChannelIdentifier>,
}

#[derive(Debug, Clone)]
struct Label {
    description: String,
    value: f32,
    colour: Color,
}

fn create_labelled_data(
    labels: Vec<Annotation>,
    acquisitions: Vec<(Acquisition, GlobalTransform)>,
    channels: &[ChannelIdentifier],
) -> (Vec<Vec<f32>>, Vec<f32>, Vec<Label>) {
    let mut classification_data = Vec::new();
    let mut classification_labels = Vec::new();

    let mut label_colours = Vec::new();

    for (label_index, annotation) in labels.iter().enumerate() {
        label_colours.push(Label {
            description: annotation.description.clone(),
            value: label_index as f32,
            colour: annotation.colour().bevy(),
        });

        for (acquisition, transform) in acquisitions.iter() {
            // Check that annotation is at least partially within the acquisition,
            // if not, then we can finish early

            // TODO: Check full bounding box, whether there is any overlap

            // If not, then we are done
            // If there is some overlap, is it complete? If so, we are also done
            // If not complete, then split the area into 4 (limited by the pixel size), and repeat

            //annotation.
            // println!(
            //     "Processing {} for annotation {} | {:?}",
            //     acquisition.description(),
            //     annotation.description,
            //     transform
            // );

            let acquisition = acquisition.mcd_acquisition();

            let width = acquisition.width() as u32;
            let height = acquisition.height() as u32;

            let mut pixels = Vec::new();

            let start = Instant::now();

            annotation.pixel_annotation(
                &PixelAnnotationConf {
                    width,
                    height,
                    transform,
                },
                (0, 0),
                (width, height),
                &mut pixels,
            );

            if pixels.is_empty() {
                continue;
            }

            // For some reason the y-axis is the wrong way up..
            let pixels: Vec<(u32, u32)> =
                pixels.iter().map(|(x, y)| (*x, height - *y - 1)).collect();

            let mut channel_indicies = Vec::new();
            for identifier in channels {
                channel_indicies.push(
                    acquisition
                        .channel(identifier)
                        .map(|channel| channel.order_number() as usize),
                );
            }

            println!(
                "Time to determine which pixels {:?}",
                Instant::now().duration_since(start)
            );

            // println!("Indicies: {:?}", channel_indicies);
            // println!("Channels: {:?}", acquisition.channels()[10]);
            // println!("Channels: {:?}", acquisition.channels()[18]);
            // println!("Channels: {:?}", channels);
            // println!(
            //     "Channels: {:?}",
            //     acquisition
            //         .channels()
            //         .iter()
            //         .map(|channel| channel.label().to_string())
            //         .collect::<Vec<String>>()
            // );

            // The following code can be used to show which pixels are included in the

            // println!("Total count: {:?}", pixels.len());

            // let mut data =
            //     vec![
            //         0;
            //         (acquisition.width() * acquisition.height()) as usize * 4
            //     ];

            // for (x, y) in &pixels {
            //     let index = ((y * width + x) * 4) as usize;

            //     data[index] = 100;
            //     data[index + 1] = 60;
            //     data[index + 2] = 150;
            //     data[index + 3] = 255;
            // }

            // let image = Image::new(
            //     Extent3d {
            //         width: acquisition.width() as u32,
            //         height: acquisition.height() as u32,
            //         depth_or_array_layers: 1,
            //     },
            //     TextureDimension::D2,
            //     data,
            //     TextureFormat::Rgba8Unorm,
            // );

            // let pixel_annotation = commands
            //     .spawn_bundle(SpriteBundle {
            //         //transform,
            //         texture: textures.add(image),
            //         sprite: Sprite {
            //             custom_size: Some(Vec2::new(
            //                 acquisition.width() as f32,
            //                 acquisition.height() as f32,
            //             )),
            //             anchor: Anchor::Center,
            //             ..Default::default()
            //         },
            //         ..Default::default()
            //     })
            //     .insert(UiEntry {
            //         description: "ANNOTATION ENTITY".to_string(),
            //     })
            //     .insert(Opacity(1.0))
            //     .id();

            // commands.entity(*acq_entity).add_child(pixel_annotation);

            // Perform random forest classification

            for (x, y) in pixels {
                let spectrum = acquisition.spectrum(x as usize, y as usize).unwrap();

                let mut to_classify = vec![0.0; channels.len()];

                for (index, channel_index) in channel_indicies.iter().enumerate() {
                    if let Some(channel_index) = channel_index {
                        to_classify[index] = spectrum[*channel_index];
                    }
                }

                classification_data.push(to_classify);
                classification_labels.push(label_index as f32);
            }
        }
    }

    (classification_data, classification_labels, label_colours)
}

struct Classifier {
    target: PixelAnnotationTarget,
    channels: Vec<ChannelIdentifier>,
    tree: Arc<DecisionTreeClassifier<f32>>,
    labels: Vec<Label>,
    output: ClassifierOutput,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassifierOutput {
    Window,
    File { location: PathBuf },
}

struct ClassificationResult {
    acq_entity: Entity,
    labels: Vec<Label>,
    region: imc_rs::Region,
    predicted_labels: Vec<f32>,
    output: ClassifierOutput,
}

#[derive(Component)]
struct BuildClassifier(Task<Classifier>);

fn apply_classifier(
    mut commands: Commands,
    mut q_classifiers: Query<(Entity, &mut BuildClassifier)>,
    q_imc: Query<(Entity, &IMCDataset)>,
) {
    let thread_pool = AsyncComputeTaskPool::get();

    for (entity, mut task) in q_classifiers.iter_mut() {
        if let Some(classifier) = future::block_on(future::poll_once(&mut task.0)) {
            commands.entity(entity).despawn();

            match &classifier.target {
                PixelAnnotationTarget::Region(region) => {
                    for (entity, imc) in q_imc.iter() {
                        for acquisition in imc.acquisitions_in(region) {
                            let acq_entity = imc
                                .acquisition_entity(AcquisitionIdentifier::Id(acquisition.id()))
                                .unwrap();

                            let region = acquisition.pixels_in(region).unwrap();
                            let tree = classifier.tree.clone();
                            let acquisition = acquisition.clone();
                            let channels = classifier.channels.clone();
                            let labels = classifier.labels.clone();
                            let output = classifier.output.clone();

                            let load_task: Task<Result<ClassificationResult, MCDError>> =
                                thread_pool.spawn(async move {
                                    // TODO: transform region to slide/imc global transform
                                    //println!("{} {:?}", acquisition.description(), region);

                                    let start = Instant::now();

                                    let images =
                                        acquisition.channel_images(&channels, Some(region))?;

                                    println!(
                                        "Time to load data {:?}",
                                        Instant::now().duration_since(start)
                                    );

                                    let start = Instant::now();

                                    let mut to_classify = Vec::new();

                                    for image in images {
                                        // TODO: Remove this copy
                                        to_classify.push(image.intensities().to_vec());
                                    }

                                    let to_classify =
                                        DenseMatrix::from_2d_vec(&to_classify).transpose();

                                    println!(
                                        "Time to create prediction matrix {:?}",
                                        Instant::now().duration_since(start)
                                    );
                                    let start = Instant::now();

                                    let predicted_labels = tree.predict(&to_classify).unwrap();

                                    println!(
                                        "Time to predict {:?}",
                                        Instant::now().duration_since(start)
                                    );

                                    Ok(ClassificationResult {
                                        acq_entity,
                                        region,
                                        labels,
                                        predicted_labels,
                                        output,
                                    })
                                });

                            commands.spawn(ComputeClassifier(load_task));

                            // println!("Time to predict {:?}", Instant::now().duration_since(start));

                            // println!("{:?}", label.unique());
                        }
                    }
                }
                PixelAnnotationTarget::Acquisitions(acquisitions) => {
                    for acq_entity in acquisitions {
                        // if let Some((imc, acquisition, transform)) =
                        //     get_imc_from_acquisition(*acq_entity, &q_imc, &q_acquisitions)
                        // {
                        //     // TODO: this could be separated out into the annotation region as it will apply to more than IMC data
                        //     // This function should just set up the parameters required (e.g. location of bounding box, # pixels width, # pixels height => if not rectangular, then what?)

                        //     // TODO: Check whether the bounding box is correct - maybe we need to use the top left and bottom right coords instead?
                        //     //acquisition.to_slide_transform()

                        //     let mut data =
                        //         vec![
                        //             0;
                        //             (acquisition.width() * acquisition.height()) as usize * 4
                        //         ];

                        //     // let mut to_classify = Vec::new();

                        //     // for channel in acquisition.channels() {
                        //     //     if channel.name() == "X"
                        //     //         || channel.name() == "Y"
                        //     //         || channel.name() == "Z"
                        //     //     {
                        //     //         continue;
                        //     //     }

                        //     //     let channel_image = acquisition
                        //     //         .channel_data(&ChannelIdentifier::Label(
                        //     //             channel.label().to_string(),
                        //     //         ))
                        //     //         .unwrap();

                        //     //     to_classify.push(channel_image.intensities().to_vec());
                        //     // }

                        //     // let to_classify = DenseMatrix::from_2d_vec(&to_classify).transpose();

                        //     // println!("{:?}", to_classify.shape());

                        //     // let label = tree.predict(&to_classify).unwrap();

                        //     // println!("{:?}", label.unique());

                        //     // for (index, label) in label.iter().enumerate() {
                        //     //     let index = (index * 4) as usize;

                        //     //     if *label == 0.0 {
                        //     //         data[index] = 50;
                        //     //         data[index + 1] = 50;
                        //     //         data[index + 2] = 200;
                        //     //         data[index + 3] = 200;
                        //     //     } else if *label == 1.0 {
                        //     //         data[index] = 50;
                        //     //         data[index + 1] = 200;
                        //     //         data[index + 2] = 50;
                        //     //         data[index + 3] = 200;
                        //     //     } else {
                        //     //         data[index] = 200;
                        //     //         data[index + 1] = 50;
                        //     //         data[index + 2] = 50;
                        //     //         data[index + 3] = 200;
                        //     //     }
                        //     // }

                        //     for y in 0..height {
                        //         for x in 0..width {
                        //             let spectrum = DenseMatrix::from_2d_vec(&vec![acquisition
                        //                 .spectrum(x as usize, y as usize)
                        //                 .unwrap()[channel_start_index..]
                        //                 .to_vec()]);

                        //             let label = tree.predict(&spectrum).unwrap();
                        //             let index = ((y * width + x) * 4) as usize;

                        //             if label[0] == 0.0 {
                        //                 data[index] = 50;
                        //                 data[index + 1] = 50;
                        //                 data[index + 2] = 200;
                        //                 data[index + 3] = 200;
                        //             } else if label[0] == 1.0 {
                        //                 data[index] = 50;
                        //                 data[index + 1] = 200;
                        //                 data[index + 2] = 50;
                        //                 data[index + 3] = 200;
                        //             } else {
                        //                 data[index] = 200;
                        //                 data[index + 1] = 50;
                        //                 data[index + 2] = 50;
                        //                 data[index + 3] = 200;
                        //             }
                        //         }
                        //     }

                        //     let image = Image::new(
                        //         Extent3d {
                        //             width: acquisition.width() as u32,
                        //             height: acquisition.height() as u32,
                        //             depth_or_array_layers: 1,
                        //         },
                        //         TextureDimension::D2,
                        //         data,
                        //         TextureFormat::Rgba8Unorm,
                        //     );

                        //     let pixel_annotation = commands
                        //         .spawn_bundle(SpriteBundle {
                        //             //transform,
                        //             texture: textures.add(image),
                        //             sprite: Sprite {
                        //                 custom_size: Some(Vec2::new(
                        //                     acquisition.width() as f32,
                        //                     acquisition.height() as f32,
                        //                 )),
                        //                 anchor: Anchor::Center,
                        //                 ..Default::default()
                        //             },
                        //             ..Default::default()
                        //         })
                        //         .insert(UiEntry {
                        //             description: "Classification results".to_string(),
                        //         })
                        //         .insert(Opacity(1.0))
                        //         .id();

                        //     commands.entity(*acq_entity).add_child(pixel_annotation);
                        // }
                    }
                }
            }
        }
    }
}

#[derive(Component)]
struct ComputeClassifier(Task<Result<ClassificationResult, MCDError>>);

fn process_classifier_results(
    mut commands: Commands,
    mut q_results: Query<(Entity, &mut ComputeClassifier)>,
    q_acquisition: Query<&Acquisition>,
    mut textures: ResMut<Assets<Image>>,
) {
    for (entity, mut task) in q_results.iter_mut() {
        if let Some(classifier) = future::block_on(future::poll_once(&mut task.0)) {
            commands.entity(entity).despawn();

            // TODO: Check error and spawn it if necessary
            if let Err(error) = classifier {
                commands.spawn(Message {
                    severity: Severity::Error,
                    message: format!("Error classifing: {}", error),
                });

                continue;
            }

            let result = classifier.unwrap();

            let acquisition = q_acquisition.get(result.acq_entity).unwrap();

            let mcd_acquisition = acquisition.mcd_acquisition();

            let region = result.region;

            match result.output {
                ClassifierOutput::Window => {
                    let mut data = vec![0; (region.width * region.height) as usize * 4];

                    for (index, label) in result.predicted_labels.iter().enumerate() {
                        let index = index * 4;

                        let label = *label as usize;
                        let colour = result.labels[label].colour;

                        data[index] = (colour.r() * 255.0) as u8;
                        data[index + 1] = (colour.g() * 255.0) as u8;
                        data[index + 2] = (colour.b() * 255.0) as u8;
                        data[index + 3] = 200;
                    }

                    let image = Image::new(
                        Extent3d {
                            width: region.width,
                            height: region.height,
                            depth_or_array_layers: 1,
                        },
                        TextureDimension::D2,
                        data,
                        TextureFormat::Rgba8Unorm,
                    );

                    let pixel_annotation = commands
                        .spawn(SpriteBundle {
                            transform: Transform::from_translation(Vec3::new(
                                (-mcd_acquisition.width() as f32 * 0.5)
                                    + region.x as f32
                                    + (region.width as f32 * 0.5),
                                (mcd_acquisition.height() as f32 * 0.5)
                                    - region.y as f32
                                    - (region.height as f32 * 0.5),
                                0.0,
                            )),
                            texture: textures.add(image),
                            sprite: Sprite {
                                custom_size: Some(Vec2::new(
                                    region.width as f32,
                                    region.height as f32,
                                )),
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
                            description: "Classification results".to_string(),
                        })
                        .insert(Opacity(1.0))
                        .id();

                    commands.entity(result.acq_entity).despawn_descendants();
                    commands
                        .entity(result.acq_entity)
                        .add_child(pixel_annotation);
                }
                ClassifierOutput::File { location } => {
                    if let Some(location) = acquisition.mcd().location() {
                        let mut mcd_location = PathBuf::from(location);
                        mcd_location.set_extension("");
                        println!("Filename: {:?}", mcd_location.file_name());

                        let mut filename: String = mcd_location
                            .file_name()
                            .and_then(|s| s.to_str())
                            .map(|s| s.to_string())
                            .unwrap();

                        write!(filename, "_{}", mcd_acquisition.description()).unwrap();
                        //let output = location.join("path")

                        if region.x != 0
                            || region.y != 0
                            || region.width != acquisition.width() as u32
                            || region.height != acquisition.height() as u32
                        {
                            // A sub region of the acquitision was classified, so include this in the name
                            write!(
                                filename,
                                "_Region_x_{}_{}_y_{}_{}",
                                region.x,
                                region.x + region.width,
                                region.y,
                                region.y + region.height
                            )
                            .unwrap();
                        }

                        let mut data = vec![
                            vec![0_u8; (region.width * region.height) as usize];
                            result.labels.len()
                        ];

                        for (index, label) in result.predicted_labels.iter().enumerate() {
                            let label = *label as usize;

                            data[label][index] = 255;
                        }

                        for (index, label) in result.labels.iter().enumerate() {
                            let mut filename = filename.clone();
                            write!(filename, "_{}", label.description).unwrap();

                            let mut location = location.join(filename);
                            location.set_extension("tiff");

                            let img_file = File::create(location).expect("Cannot find test image!");
                            let mut writer = BufWriter::new(img_file);

                            let mut tiff = TiffEncoder::new(&mut writer).unwrap();

                            tiff.write_image::<colortype::Gray8>(
                                region.width,
                                region.height,
                                &data[index],
                            )
                            .unwrap();
                        }
                    }
                }
            }
        }
    }
}

#[derive(Component)]
pub(crate) struct LoadIMC(pub Task<Result<MCD<File>, MCDError>>);

#[derive(Component)]
pub struct Slide {
    pub id: u16,
}

#[derive(Component)]
pub struct Panorama {}

#[derive(Component, Clone)]
pub struct Acquisition {
    mcd: Arc<MCD<File>>,
    imc_dataset: Entity,

    id: u16,
}

impl Acquisition {
    fn mcd(&self) -> &MCD<File> {
        &self.mcd
    }

    fn mcd_acquisition(&self) -> &imc_rs::Acquisition<File> {
        self.mcd
            .acquisition(AcquisitionIdentifier::Id(self.id))
            .unwrap()
    }

    fn width(&self) -> i32 {
        self.mcd_acquisition().width()
    }

    fn height(&self) -> i32 {
        self.mcd_acquisition().height()
    }
}

impl From<MCDError> for Message {
    fn from(error: MCDError) -> Self {
        Self {
            severity: Severity::Error,
            message: error.to_string(),
        }
    }
}

fn load_imc(
    mut commands: Commands,
    mut q_imc: Query<(Entity, &mut LoadIMC)>,
    mut textures: ResMut<Assets<Image>>,
) {
    let thread_pool = AsyncComputeTaskPool::get();

    for (entity, mut task) in q_imc.iter_mut() {
        if let Some(result) = future::block_on(future::poll_once(&mut task.0)) {
            commands.entity(entity).remove::<LoadIMC>();

            match result {
                Err(error) => {
                    commands.entity(entity).insert(Message::from(error));
                }
                Ok(mcd) => {
                    // let xml = mcd.xml().unwrap();
                    // std::fs::write("mcd.xml", xml).unwrap();

                    let mcd = Arc::new(mcd);

                    let mut panoramas = Vec::new();
                    let mut acquisition_entities = HashMap::new();

                    commands
                        .entity(entity)
                        .with_children(|parent| {
                            for slide in mcd.slides() {
                                parent
                                    .spawn(SpatialBundle {
                                        transform: Transform::from_xyz(0.0, 0.0, 1.0),
                                        ..Default::default()
                                    })
                                    .insert(Slide { id: slide.id() })
                                    .insert(UiEntry {
                                        description: slide.description().to_owned(),
                                    })
                                    .with_children(|parent| {
                                        // Load in the slide image
                                        let image = slide.image();
                                        let slide_width = slide.width_in_um() as f32;
                                        let slide_height = slide.height_in_um() as f32;

                                        let image_task = thread_pool.spawn(async move {
                                            let image = image.as_rgba8()?;

                                            let tile_width = 512;
                                            let tile_height = 512;

                                            Ok(ToTileImage {
                                                image,
                                                tile_width,
                                                tile_height,
                                                image_width: slide_width,
                                                image_height: slide_height,
                                            })
                                        });

                                        parent
                                            .spawn((
                                                UiEntry {
                                                    description: format!(
                                                        "{} (optical image)",
                                                        slide.description()
                                                    ),
                                                },
                                                Draggable,
                                                Opacity(1.0),
                                                TiledImage {
                                                    size: Vec2::new(
                                                        slide.width_in_um() as f32,
                                                        slide.height_in_um() as f32,
                                                    ),
                                                },
                                                BoundingBox {
                                                    x: 0.0,
                                                    y: 0.0,
                                                    width: slide.width_in_um() as f32,
                                                    height: slide.height_in_um() as f32,
                                                },
                                                SpatialBundle {
                                                    transform: Transform::from_xyz(
                                                        slide.width_in_um() as f32 / 2.0,
                                                        slide.height_in_um() as f32 / 2.0,
                                                        0.0,
                                                    ),
                                                    ..Default::default()
                                                },
                                            ))
                                            .insert(ComputeTileImage(image_task));
                                        //.with_children(|parent| {});
                                    })
                                    .with_children(|parent| {
                                        for (index, panorama) in
                                            slide.panoramas().iter().enumerate()
                                        {
                                            let panorama_dimensions = panorama.dimensions();

                                            let panorama_bounding_box =
                                                panorama.slide_bounding_box();

                                            println!(
                                                "Panorama [{:?}] {:?}",
                                                panorama_dimensions, panorama_bounding_box
                                            );

                                            // Now add in the tranformation for the overview image
                                            let mut mat = imc_transform_to_matrix4(
                                                panorama.to_slide_transform(),
                                            )
                                            .unwrap();
                                            // Make sure that panorama is infront of slide
                                            mat.m34 = 1.0;
                                            let panorama_transform = AffineTransform::new(
                                                "panorama_transform".to_string(),
                                                mat,
                                            );
                                            //panorama.

                                            // Transformation is defined with respect to slide, so make sure to offset properly
                                            let transform =
                                                create_transform(
                                                    &panorama_transform,
                                                    panorama_dimensions.0 as f32,
                                                    panorama_dimensions.1 as f32,
                                                    false,
                                                )
                                                .mul_transform(Transform::from_translation(
                                                    Vec3::new(0.0, 0.0, index as f32),
                                                ));

                                            let inverse_panorama = Transform::from_matrix(
                                                transform.compute_matrix().inverse(),
                                            );

                                            println!("Panorama transform = {:?}", transform);

                                            let mut panorama_commands =
                                                parent.spawn(SpatialBundle {
                                                    //texture: texture_handle,
                                                    transform,
                                                    // sprite: Sprite {
                                                    //     custom_size: Some(Vec2::new(
                                                    //         panorama_dimensions.0 as f32,
                                                    //         panorama_dimensions.1 as f32,
                                                    //     )),
                                                    //     anchor: Anchor::Center,
                                                    //     ..Default::default()
                                                    // },
                                                    ..Default::default()
                                                });

                                            panorama_commands
                                                .insert(UiEntry {
                                                    description: panorama.description().to_owned(),
                                                })
                                                .insert(Draggable)
                                                .insert(Opacity(1.0))
                                                .insert(Panorama {})
                                                .with_children(|parent| {
                                                    for acquisition in panorama.acquisitions() {
                                                        // Now add in the tranformation for the overview image
                                                        let mut mat = imc_transform_to_matrix4(
                                                            acquisition.to_slide_transform(),
                                                        )
                                                        .unwrap();
                                                        // Make sure that panorama is infront of slide
                                                        mat.m34 = index as f64 + 1.5;
                                                        let panorama_transform =
                                                            AffineTransform::new(
                                                                "acquisition_transform".to_string(),
                                                                mat,
                                                            );

                                                        let transform = inverse_panorama
                                                            .mul_transform(create_transform(
                                                                &panorama_transform,
                                                                acquisition.width() as f32,
                                                                acquisition.height() as f32,
                                                                false,
                                                            ));

                                                        let data = vec![
                                                            0;
                                                            (acquisition.width()
                                                                * acquisition.height())
                                                                as usize
                                                                * 4
                                                        ];
                                                        let image = Image::new(
                                                            Extent3d {
                                                                width: acquisition.width() as u32,
                                                                height: acquisition.height() as u32,
                                                                depth_or_array_layers: 1,
                                                            },
                                                            TextureDimension::D2,
                                                            data,
                                                            TextureFormat::Rgba8Unorm,
                                                        );

                                                        let acquisition_entity = parent
                                                            .spawn(SpriteBundle {
                                                                transform,
                                                                texture: textures.add(image),
                                                                sprite: Sprite {
                                                                    custom_size: Some(Vec2::new(
                                                                        acquisition.width() as f32,
                                                                        acquisition.height() as f32,
                                                                    )),
                                                                    anchor: Anchor::Center,
                                                                    ..Default::default()
                                                                },
                                                                ..Default::default()
                                                            })
                                                            .insert(UiEntry {
                                                                description: acquisition
                                                                    .description()
                                                                    .to_owned(),
                                                            })
                                                            .insert(Acquisition {
                                                                id: acquisition.id(),
                                                                mcd: mcd.clone(),
                                                                imc_dataset: entity,
                                                            })
                                                            .insert(Opacity(1.0))
                                                            .id();

                                                        acquisition_entities.insert(
                                                            acquisition.id(),
                                                            acquisition_entity,
                                                        );
                                                    }
                                                });

                                            // Load in the slide image
                                            if let Some(panorama_image) = panorama.image() {
                                                let image_task = thread_pool.spawn(async move {
                                                    let image = panorama_image.as_rgba8()?;

                                                    let tile_width = 1024;
                                                    let tile_height = 1024;

                                                    Ok(ToTileImage {
                                                        image,
                                                        tile_width,
                                                        tile_height,
                                                        image_width: panorama_dimensions.0 as f32,
                                                        image_height: panorama_dimensions.1 as f32,
                                                    })
                                                });

                                                panorama_commands
                                                    .insert(ComputeTileImage(image_task));
                                            }

                                            let panorama_entity = panorama_commands.id();

                                            panoramas.push(panorama_entity);
                                        }
                                    });
                            }

                            // Now add in a new control for the images
                            parent.spawn(ImageControl {
                                description: "Red Channel".to_string(),
                                entities: acquisition_entities.clone(),
                                intensity_range: (0.0, 0.0),
                                image_update_type: ImageUpdateType::Red,
                                histogram: Vec::new(),
                                colour_domain: (0.0, 0.0),
                            });
                            parent.spawn(ImageControl {
                                description: "Green Channel".to_string(),
                                entities: acquisition_entities.clone(),
                                intensity_range: (0.0, 0.0),
                                image_update_type: ImageUpdateType::Green,
                                histogram: Vec::new(),
                                colour_domain: (0.0, 0.0),
                            });
                            parent.spawn(ImageControl {
                                description: "Blue Channel".to_string(),
                                entities: acquisition_entities.clone(),
                                intensity_range: (0.0, 0.0),
                                image_update_type: ImageUpdateType::Blue,
                                histogram: Vec::new(),
                                colour_domain: (0.0, 0.0),
                            });
                        })
                        .insert(PrimaryUiEntry {
                            description: format!("IMC: {:?}", mcd.location()),
                        })
                        .insert(IMCDataset {
                            mcd,
                            histogram_scale: HistogramScale::None,
                            background_alpha: 1.0,
                            panoramas,
                            acquisitions: acquisition_entities.into_iter().collect(),
                        })
                        .insert(SpatialBundle::default());
                }
            }
        }
    }
}

#[derive(TypeUuid, Deref)]
#[uuid = "7c9402ad-cf99-4fe9-87a9-f8f45cdc8a2b"]
pub struct ChannelImage(imc_rs::ChannelImage);

// impl ChannelImage {
//     pub fn width(&self) -> usize {
//         self.0.width() as usize
//     }

//     pub fn height(&self) -> usize {
//         self.0.height() as usize
//     }

//     pub fn intensity_range(&self) -> (f32, f32) {
//         self.0.intensity_range()
//     }

//     pub fn intensities(&self) -> &[f32] {
//         self.0.intensities()
//     }
// }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistogramScale {
    None,
    Log10,
    Ln,
}

#[derive(Component, Clone)]
pub struct IMCDataset {
    mcd: Arc<MCD<File>>,

    // Settings
    histogram_scale: HistogramScale,
    background_alpha: f32,

    pub panoramas: Vec<Entity>,
    pub acquisitions: HashMap<u16, Entity>,
}

impl IMCDataset {
    pub fn name(&self) -> &str {
        self.mcd
            .location()
            .map(|path| path.to_str().unwrap_or("Unknown name"))
            .unwrap_or("Unknown name")
    }

    pub fn background_alpha(&self) -> f32 {
        self.background_alpha
    }
    pub fn histogram_scale(&self) -> &HistogramScale {
        &self.histogram_scale
    }

    pub fn acquisition(
        &self,
        identifier: AcquisitionIdentifier,
    ) -> Option<&imc_rs::Acquisition<File>> {
        self.mcd.acquisition(identifier)
    }

    pub fn acquisition_entity(&self, identifier: AcquisitionIdentifier) -> Option<Entity> {
        let acquisition = self.mcd.acquisition(identifier)?;

        self.acquisitions.get(&acquisition.id()).copied()
    }

    pub fn acquisitions(&self) -> Vec<&imc_rs::Acquisition<File>> {
        self.mcd.acquisitions()
    }

    pub fn channels(&self) -> Vec<&AcquisitionChannel> {
        self.mcd.channels()
    }

    pub fn channel_image(
        &self,
        identifier: &ChannelIdentifier,
    ) -> Result<HashMap<u16, ChannelImage>, MCDError> {
        let mut image_map = HashMap::new();

        for acquisition in self.mcd.acquisitions() {
            match acquisition.channel_image(identifier, None) {
                Ok(data) => {
                    image_map.insert(acquisition.id(), ChannelImage(data));
                }
                Err(MCDError::InvalidChannel { channel: _ }) => {
                    // This channel doesn't exist for this acquisition (can happen sometimes if the panel was changed),
                    // so we just ignore this error
                }
                Err(error) => return Err(error),
            }
        }

        Ok(image_map)
    }

    pub fn acquisitions_in(
        &self,
        region: &imc_rs::BoundingBox<f64>,
    ) -> Vec<&imc_rs::Acquisition<File>> {
        self.mcd.acquisitions_in(region)
    }
}

pub fn imc_transform_to_matrix4<T: imc_rs::transform::TransformScalar>(
    transform: imc_rs::transform::AffineTransform<T>,
) -> Option<Matrix4<T>> {
    let matrix3 = transform.to_slide_matrix()?;

    Some(Matrix4::new(
        matrix3.m11,
        matrix3.m12,
        T::zero(),
        matrix3.m13,
        matrix3.m21,
        matrix3.m22,
        T::zero(),
        matrix3.m23,
        T::zero(),
        T::zero(),
        T::one(),
        T::zero(),
        T::zero(),
        T::zero(),
        T::zero(),
        T::one(),
    ))
}

#[derive(Component)]
struct AcquisitionChannelImage {
    acquisition_entity: Entity,
    data: Option<Handle<ChannelImage>>,
}

// TODO: Should this be part of the ImagePlugin?
fn generate_histogram(
    mut q_control: Query<(&mut ImageControl, &Children)>,
    q_acquisition_images: Query<(Entity, &AcquisitionChannelImage)>,
    channel_data: Res<Assets<ChannelImage>>,
) {
    for (mut control, children) in q_control.iter_mut() {
        if control.histogram.is_empty() {
            // Need to create the histogram
            let num_bins = 100;
            let mut histogram = vec![0; num_bins];

            // println!("{:?}", control.intensity_range);

            let bin_size =
                (control.intensity_range.1 - control.intensity_range.0) / (num_bins - 1) as f32;
            // println!("[control] Query: {:?}", children);
            // println!(
            //     "Query: {:?}",
            //     q_acquisition_images
            //         .iter()
            //         .map(|(entity, _)| entity)
            //         .collect::<Vec<_>>()
            // );

            for child in children.iter() {
                if let Ok((_, acq_channel_image)) = q_acquisition_images.get(*child) {
                    if let Some(data) = &acq_channel_image.data {
                        if let Some(channel_image) = channel_data.get(data) {
                            for intensity in channel_image.0.intensities() {
                                let index = ((intensity - control.intensity_range.0) / bin_size)
                                    .floor() as usize;

                                if index >= histogram.len() {
                                    error!(
                                        "We have a problem generating histogram ({}): {} | {:?}",
                                        channel_image.0.name(),
                                        intensity,
                                        control.intensity_range
                                    );
                                    break;
                                } else {
                                    histogram[index] += 1;
                                }
                            }
                        }
                    }
                }
            }

            // Set the colour domain to be the 99th percentile
            let total = histogram.iter().sum::<usize>() as f64;
            let mut bin = 0;
            let mut running_total = 0;
            for (index, value) in histogram.iter().enumerate() {
                running_total += value;
                bin = index;

                if running_total as f64 / total >= 0.995 {
                    break;
                }
            }

            control.colour_domain = (0.0, bin_size * (bin + 1) as f32 + control.intensity_range.0);
            control.histogram = histogram;
        }
    }
}

// This needs rethinking. Ideally want to generate the mixture between the various
fn image_control_changed(
    q_imc: Query<(&IMCDataset, &Children, ChangeTrackers<IMCDataset>)>,
    q_control: Query<(&ImageControl, &Children, ChangeTrackers<ImageControl>)>,
    q_acquisition: Query<&Handle<Image>, With<Acquisition>>,
    q_acquisition_images: Query<&AcquisitionChannelImage>,
    channel_data: Res<Assets<ChannelImage>>,
    mut textures: ResMut<Assets<Image>>,
) {
    for (imc, children, imc_tracker) in q_imc.iter() {
        let requires_update = imc_tracker.is_changed();

        for (control_index, (control, children, control_tracker)) in q_control.iter().enumerate() {
            // Check each AcquisitionChannelImage (child of ImageControl)

            if control_tracker.is_changed() || requires_update {
                info!(
                    "Channel image updated => recalculating | {:?}",
                    imc.background_alpha()
                );

                for child in children.iter() {
                    if let Ok(acq_channel_image) = q_acquisition_images.get(*child) {
                        if let Ok(image) = q_acquisition.get(acq_channel_image.acquisition_entity) {
                            if let Some(image) = textures.get_mut(image) {
                                match &acq_channel_image.data {
                                    Some(data) => {
                                        if let Some(channel_image) = channel_data.get(data) {
                                            for (index, intensity) in
                                                channel_image.intensities().iter().enumerate()
                                            {
                                                if control_index == 0 {
                                                    image.data[index * 4 + 3] =
                                                        (imc.background_alpha() * 255.0) as u8;
                                                }

                                                let intensity = ((intensity
                                                    - control.colour_domain.0)
                                                    / (control.colour_domain.1
                                                        - control.colour_domain.0)
                                                    * 255.0)
                                                    as u8;

                                                match control.image_update_type {
                                                    ImageUpdateType::Red => {
                                                        image.data[index * 4] = intensity;
                                                    }
                                                    ImageUpdateType::Green => {
                                                        image.data[index * 4 + 1] = intensity;
                                                    }
                                                    ImageUpdateType::Blue => {
                                                        image.data[index * 4 + 2] = intensity;
                                                    }
                                                    ImageUpdateType::All => {
                                                        image.data[index * 4] = intensity;
                                                        image.data[index * 4 + 1] = intensity;
                                                        image.data[index * 4 + 2] = intensity;
                                                    }
                                                }

                                                // let intensity = image.data[index * 4 + 2]
                                                //     .max(image.data[index * 4 + 1])
                                                //     .max(image.data[index * 4]);
                                                // let alpha = match intensity {
                                                //     0..=25 => intensity * 10,
                                                //     _ => 255,
                                                // };

                                                // image.data[index * 4 + 3] = alpha;
                                                if intensity > 0 {
                                                    image.data[index * 4 + 3] = 255;
                                                }
                                            }
                                        }
                                    }
                                    None => {
                                        // // There is no data associated with this acquisition, so remove the previous data
                                        // for chunk in image.data.chunks_mut(4) {
                                        //     match control.image_update_type {
                                        //         ImageUpdateType::Red => {
                                        //             chunk[0] = 0;
                                        //         }
                                        //         ImageUpdateType::Green => {
                                        //             chunk[1] = 0;
                                        //         }
                                        //         ImageUpdateType::Blue => {
                                        //             chunk[2] = 0;
                                        //         }
                                        //         ImageUpdateType::All => {
                                        //             chunk[0] = 0;
                                        //             chunk[1] = 0;
                                        //             chunk[2] = 0;
                                        //         }
                                        //     }

                                        //     let intensity = chunk[2].max(chunk[1].max(chunk[0]));
                                        //     let alpha = match intensity {
                                        //         0..=25 => intensity * 10,
                                        //         _ => 255,
                                        //     };

                                        //     chunk[3] = alpha;
                                        // }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn generate_channel_image(
    mut commands: Commands,
    mut q_generate: Query<(
        Entity,
        &mut ImageControl,
        &GenerateChannelImage,
        &Parent,
        //&Children,
    )>,
    q_acquisition: Query<&Acquisition>,
    q_imc: Query<&IMCDataset>, //&GenerateChannelImage
    mut channel_data: ResMut<Assets<ChannelImage>>,
    //mut textures: ResMut<Assets<Image>>,
) {
    for (entity, mut image_control, generate, parent) in q_generate.iter_mut() {
        // Remove children from the image control (previously loaded data)
        commands.entity(entity).despawn_descendants();

        // We are generating the channel image, so we can remove this
        commands.entity(entity).remove::<GenerateChannelImage>();

        if let Ok(imc) = q_imc.get(parent.get()) {
            let start = Instant::now();

            let Some(identifier) = &generate.identifier else {
                image_control.histogram = vec![];
                image_control.intensity_range = (0.0, f32::INFINITY);
                image_control.colour_domain = (0.0, f32::INFINITY);

                continue;
            };

            match imc.channel_image(identifier) {
                Ok(mut channel_images) => {
                    let duration = start.elapsed();

                    println!("Time elapsed loading data is: {:?}", duration);
                    let mut min_value = f32::MAX;
                    let mut max_value = f32::MIN;
                    let mut image_entities = HashSet::with_capacity(channel_images.len());

                    for (acq_id, acquisition_entity) in image_control.entities.iter() {
                        if let Ok(acquisition) = q_acquisition.get(*acquisition_entity) {
                            if let Some(channel_image) = channel_images.remove(&acquisition.id) {
                                // If the image is empty, then we don't need to do anything
                                if channel_image.width() == 0 || channel_image.height() == 0 {
                                    continue;
                                }

                                let image_range = channel_image.intensity_range();

                                if image_range.0 < min_value {
                                    min_value = image_range.0
                                }
                                if image_range.0 > max_value {
                                    max_value = image_range.0
                                }
                                if image_range.1 < min_value {
                                    min_value = image_range.1
                                }
                                if image_range.1 > max_value {
                                    max_value = image_range.1
                                }

                                let channel_image_entity = commands
                                    .spawn(AcquisitionChannelImage {
                                        acquisition_entity: *acquisition_entity,
                                        data: Some(channel_data.add(channel_image)),
                                    })
                                    .id();

                                image_entities.insert(channel_image_entity);

                                commands.entity(entity).add_child(channel_image_entity);
                            } else {
                                let channel_image_entity = commands
                                    .spawn(AcquisitionChannelImage {
                                        acquisition_entity: *acquisition_entity,
                                        data: None,
                                    })
                                    .id();

                                image_entities.insert(channel_image_entity);

                                commands.entity(entity).add_child(channel_image_entity);
                            }
                        }
                    }

                    image_control.histogram = vec![];
                    image_control.intensity_range = (min_value, max_value);
                    image_control.colour_domain = (min_value, max_value);
                }
                Err(error) => {
                    commands.spawn(Message {
                        severity: Severity::Error,
                        message: format!("Failed to load channel data: {}", error),
                    });
                }
            }
        }
    }
}
