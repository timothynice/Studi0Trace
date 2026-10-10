//! macOS Vision's text recogniser as the finder: accurate level, no language correction (a wordmark is not a
//! word), every text height. The image goes in as PNG bytes; the observations come back as normalised
//! rectangles with the origin at the bottom left, turned into source-pixel quads here.
use super::error;
use super::reader::{Finder, LineBox};
use crate::error::CommandError;
use crate::redraw::Source;

pub struct Vision;

#[cfg(target_os = "macos")]
impl Finder for Vision {
    fn find(&self, source: &Source) -> Result<Vec<LineBox>, CommandError> {
        // the Objective-C objects autoreleased here are let go of when the call ends, not when the blocking-pool
        // thread that ran it does (it never drains a pool of its own)
        objc2::rc::autoreleasepool(|_| find_lines(source))
    }
}

#[cfg(target_os = "macos")]
fn find_lines(source: &Source) -> Result<Vec<LineBox>, CommandError> {
    use super::VISION_MIN_CONFIDENCE;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::AllocAnyThread;
    use objc2_foundation::{NSArray, NSData, NSDictionary};
    use objc2_vision::{VNImageOption, VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel};

    let png = crate::redraw::encode_png(&source.rgba, source.width, source.height, image::ExtendedColorType::Rgba8)?;
    let data = NSData::with_bytes(&png);
    let options: Retained<NSDictionary<VNImageOption, AnyObject>> = NSDictionary::new();
    let handler = VNImageRequestHandler::initWithData_options(VNImageRequestHandler::alloc(), &data, &options);
    let request = VNRecognizeTextRequest::new();
    request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
    request.setUsesLanguageCorrection(false);
    request.setMinimumTextHeight(0.0);
    let as_request: Retained<VNRequest> = Retained::into_super(Retained::into_super(request.clone()));
    let requests = NSArray::from_retained_slice(&[as_request]);
    // Vision is there (the app needs macOS 13) and the request failed: not "unavailable"
    handler.performRequests_error(&requests).map_err(|e| {
        eprintln!("studi0trace: Vision failed: {e}");
        error("text_failed")
    })?;
    let (w, h) = (source.width as f32, source.height as f32);
    let mut lines = Vec::new();
    if let Some(results) = request.results() {
        for obs in results.iter() {
            let Some(best) = obs.topCandidates(1).iter().next() else { continue };
            let string = best.string().to_string();
            let confidence = best.confidence();
            if string.trim().is_empty() || confidence < VISION_MIN_CONFIDENCE {
                continue;
            }
            let px = |p: objc2_core_foundation::CGPoint| [p.x as f32 * w, (1.0 - p.y as f32) * h];
            let quad = unsafe { [px(obs.topLeft()), px(obs.topRight()), px(obs.bottomRight()), px(obs.bottomLeft())] };
            lines.push(LineBox { quad, string, confidence });
        }
    }
    // top to bottom, then left to right
    lines.sort_by(|a, b| a.quad[0][1].total_cmp(&b.quad[0][1]).then(a.quad[0][0].total_cmp(&b.quad[0][0])));
    Ok(lines)
}

#[cfg(not(target_os = "macos"))]
impl Finder for Vision {
    fn find(&self, _source: &Source) -> Result<Vec<LineBox>, CommandError> {
        Err(error("text_unavailable"))
    }
}
