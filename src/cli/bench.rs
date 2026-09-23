//! `aurion bench`: measure the CPU cost of every image treatment on the
//! current machine (run it on the Raspberry Pi to size the battery).

use std::time::Instant;

use crate::core::config::AppConfig;
use crate::core::denoise::{remove_hot_pixels, synthetic_sky, Stacker};
use crate::core::detection::AuroraDetector;
use crate::core::jpeg;
use crate::core::orchestrator::encode_jpeg;

fn time<T>(label: &str, rows: &mut Vec<(String, f64)>, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let out = f();
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    rows.push((label.to_string(), ms));
    out
}

/// Run the benchmark on a synthetic frame of `width`×`height` (default:
/// IMX477 full resolution 4056×3040).
pub fn run_bench(width: u32, height: u32) -> anyhow::Result<()> {
    println!("Aurion bench : image synthétique {}×{} ({:.1} Mpx)", width, height, (width * height) as f64 / 1e6);
    let mut rows = Vec::new();
    let (_, noisy) = synthetic_sky(width, height, 6.0, 500, 1);

    let full_jpeg = time("Encodage JPEG pleine résolution (q92)", &mut rows, || encode_jpeg(&noisy, width, height))
        .ok_or_else(|| anyhow::anyhow!("encodage impossible"))?;
    let small = image::imageops::thumbnail(
        &image::RgbImage::from_raw(width, height, noisy.clone()).expect("size"),
        320,
        240,
    );
    let thumb = encode_jpeg(small.as_raw(), small.width(), small.height()).expect("thumb");
    let capture = jpeg::insert_segment(&full_jpeg, &jpeg::build_exif_with_thumbnail(&thumb, true));

    let (thumb_rgb, tw, th, _) = time("Analyse : lecture de la miniature EXIF", &mut rows, || {
        jpeg::decode_for_analysis(&capture, 640).expect("decode")
    });
    let mut full_rgb = time("Décodage JPEG pleine résolution", &mut rows, || {
        image::load_from_memory(&capture).expect("decode").to_rgb8().into_raw()
    });

    let cfg = AppConfig::default();
    time("Détection sur la miniature", &mut rows, || {
        AuroraDetector::from_config(&cfg.detection).analyze(&thumb_rgb, tw, th)
    });
    time("Détection sur l'image complète (ancienne méthode)", &mut rows, || {
        AuroraDetector::from_config(&cfg.detection).analyze(&full_rgb, width, height)
    });
    let fixed = time("Suppression des pixels chauds", &mut rows, || remove_hot_pixels(&mut full_rgb, width, height, 40));
    time("Empilement : ajout d'une image", &mut rows, || {
        let mut s = Stacker::new(width, height);
        s.add(&full_rgb);
        s
    });
    let mut stack = Stacker::new(width, height);
    for _ in 0..4 {
        stack.add(&full_rgb);
    }
    time("Empilement : calcul du résultat (4 images)", &mut rows, || stack.result());

    println!();
    for (label, ms) in &rows {
        println!("  {:<52} {:>9.1} ms", label, ms);
    }
    println!("\n  Pixels chauds corrigés : {}", fixed);
    println!("  Coût par image enregistrée avec correction des pixels chauds ≈ décodage + correction + encodage.");
    Ok(())
}
