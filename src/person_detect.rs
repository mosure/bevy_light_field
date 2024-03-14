use std::cmp::{max, min};

use bevy::prelude::*;
use image::{ImageBuffer, Luma};

use crate::{
    matting::MattedStream,
    stream::StreamId,
};


pub struct PersonDetectPlugin;

impl Plugin for PersonDetectPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<PersonDetectedEvent>();
        app.add_systems(Update, detect_person);
    }
}


#[derive(Component)]
pub struct DetectPersons;


#[derive(Debug, Clone, Reflect, PartialEq)]
pub struct BoundingBox {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Event, Debug, Reflect, Clone)]
pub struct PersonDetectedEvent {
    pub stream_id: StreamId,
    pub bounding_box: BoundingBox,
    pub mask_sum: f32,
}


fn detect_person(
    mut ev_asset: EventReader<AssetEvent<Image>>,
    mut ev_person_detected: EventWriter<PersonDetectedEvent>,
    person_detect_streams: Query<(
        &MattedStream,
        &DetectPersons,
    )>,
    images: Res<Assets<Image>>,
) {
    for ev in ev_asset.read() {
        if let AssetEvent::Modified { id } = ev {
            for (matted_stream, _) in person_detect_streams.iter() {
                if &matted_stream.output.id() == id {
                    let image = images.get(&matted_stream.output).unwrap();

                    let buffer = ImageBuffer::<Luma<u8>, Vec<u8>>::from_raw(
                        image.width(),
                        image.height(),
                        image.data.clone(),
                    ).unwrap();

                    let bounding_box = masked_bounding_box(&buffer);
                    let sum = sum_masked_pixels(&buffer);

                    let masked_ratio = sum / (buffer.width() * buffer.height()) as f32;
                    let person_detected = masked_ratio > 0.14;

                    if person_detected {
                        ev_person_detected.send(PersonDetectedEvent {
                            stream_id: matted_stream.stream_id,
                            bounding_box: bounding_box.unwrap(),
                            mask_sum: sum,
                        });
                    }
                }
            }
        }
    }
}



pub fn masked_bounding_box(buffer: &ImageBuffer<Luma<u8>, Vec<u8>>) -> Option<BoundingBox> {
    let bounding_boxes = buffer.enumerate_pixels()
        .filter_map(|(x, y, pixel)| {
            if pixel.0[0] > 250 {
                Some((x as i32, y as i32, x as i32, y as i32))
            } else {
                None
            }
        })
        .reduce(|(
            min_x1,
            min_y1,
            max_x1,
            max_y1,
        ), (
            min_x2,
            min_y2,
            max_x2,
            max_y2,
        )| {
            (
                min(min_x1, min_x2),
                min(min_y1, min_y2),
                max(max_x1, max_x2),
                max(max_y1, max_y2),
            )
        });

    bounding_boxes.map(|(
        min_x,
        min_y,
        max_x,
        max_y
    )| {
        BoundingBox {
            x: min_x,
            y: min_y,
            width: max_x - min_x + 1,
            height: max_y - min_y + 1,
        }
    })
}


pub fn sum_masked_pixels(image: &ImageBuffer<Luma<u8>, Vec<u8>>) -> f32 {
    image.pixels()
        .map(|pixel| {
            pixel.0[0] as f32 / 255.0
        })
        .sum()
}



#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Luma};
    use approx::assert_relative_eq;


    #[test]
    fn test_masked_bounding_box() {
        let width = 10;
        let height = 10;
        let mut img: ImageBuffer<Luma<u8>, Vec<u8>> = ImageBuffer::new(width, height);

        for x in 2..=5 {
            for y in 2..=5 {
                img.put_pixel(x, y, Luma([200]));
            }
        }

        let result = masked_bounding_box(&img).expect("expected a bounding box");

        let expected = BoundingBox {
            x:2,
            y: 2,
            width: 4,
            height: 4,
        };
        assert_eq!(result, expected, "the computed bounding box did not match the expected values.");
    }


    #[test]
    fn test_sum_masked_pixels() {
        let width = 4;
        let height = 4;
        let mut img: ImageBuffer<Luma<u8>, Vec<u8>> = ImageBuffer::new(width, height);

        img.put_pixel(0, 0, Luma([255]));
        img.put_pixel(1, 0, Luma([127]));
        img.put_pixel(2, 0, Luma([63]));

        let result = sum_masked_pixels(&img);

        let expected = (255.0 + 127.0 + 63.0) / 255.0;
        assert_relative_eq!(result, expected);
    }
}
