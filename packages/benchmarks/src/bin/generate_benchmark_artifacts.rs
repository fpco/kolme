use anyhow::Context as _;
use plotters::{
    chart::ChartBuilder,
    prelude::{IntoDrawingArea, Rectangle, Text},
    style::{Color as _, ShapeStyle, TextStyle, BLACK, BLUE, RED, WHITE},
};
use plotters_svg::SVGBackend;
use regex::Regex;
use std::{
    collections::BTreeMap,
    fs::DirEntry,
    path::{Path, PathBuf},
};

#[derive(Hash, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
struct Category {
    map_size: usize,
    content_size: usize,
}

#[derive(serde::Serialize)]
struct Measurements {
    backend: String,
    measures: Vec<Measurement>,
}

#[derive(Clone, serde::Serialize)]
struct Measurement {
    reserialization_label: Option<usize>,
    measure: f64,
}

fn extract_matching_dirs<'a>(
    regex: &'a Regex,
    dir: &Path,
) -> anyhow::Result<impl Iterator<Item = Result<DirEntry, std::io::Error>> + use<'a>> {
    Ok(std::fs::read_dir(dir)
        .with_context(|| format!("Unable to traverse directory {}", dir.display()))?
        .filter(|dir| {
            if let Ok(dir) = dir {
                return regex.is_match(&dir.file_name().to_string_lossy());
            }
            false
        }))
}

fn read_measures(dir: &Path) -> anyhow::Result<f64> {
    #[derive(serde::Deserialize)]
    struct Mean {
        point_estimate: f64,
    }
    #[derive(serde::Deserialize)]
    struct Measures {
        mean: Mean,
    }

    let measures_file = dir.join("new/estimates.json");
    let content = std::fs::read_to_string(&measures_file).context("Unable to read estimates")?;
    let measures =
        serde_json::from_str::<Measures>(&content).context("Unable to deserialize estimates")?;

    Ok(measures.mean.point_estimate)
}

fn calculate_scale(measure: f64) -> (f64, &'static str) {
    if measure > 1_000_000_000f64 {
        (1_000_000_000f64, "s")
    } else if measure > 1_000_000f64 {
        (1_000_000f64, "ms")
    } else if measure > 1_000f64 {
        (1_000f64, "μs")
    } else {
        (1f64, "ns")
    }
}

fn format_measure(measure: f64) -> String {
    if measure >= 100.0f64 {
        format!("{:.0}", measure)
    } else if measure >= 10.0f64 {
        format!("{:.1}", measure)
    } else if measure >= 1.0f64 {
        format!("{:.2}", measure)
    } else {
        format!("{:.3}", measure).trim_matches('0').to_owned()
    }
}
fn main() -> anyhow::Result<()> {
    let percent_regex = Regex::new(r#"(\d+)%"#).expect("Unable to build percent regex");
    let groups = [4, 16, 256, 4096, 65535]
        .into_iter()
        .flat_map(|map_size| {
            [64, 256, 1024, 10240, 20480]
                .into_iter()
                .map(move |content_size| (map_size, content_size))
        })
        .collect::<Vec<_>>();
    let workspace_toml_file = PathBuf::from(
        String::from_utf8(
            std::process::Command::new(env!("CARGO"))
                .arg("locate-project")
                .arg("--workspace")
                .arg("--message-format=plain")
                .output()
                .context("Unable to spawn cargo command")?
                .stdout,
        )
        .context("Got invalid workspace from cargo command")?,
    );

    let workspace_dir = workspace_toml_file
        .parent()
        .expect("Cargo.toml file is on root directory and that is unexpected");

    let initial_insertion_dir = workspace_dir.join("target/criterion/Initial insertion");
    let reserialization_dir = workspace_dir.join("target/criterion/Reserialization");
    let mut measurements = BTreeMap::new();

    for (map_size, content_size) in groups {
        let category = format!(r#"\[{}\] \[{}\]"#, map_size, content_size);
        let regex = Regex::new(&category).unwrap();
        let matching_initial_insertion = extract_matching_dirs(&regex, &initial_insertion_dir)?
            .collect::<Result<Vec<_>, _>>()
            .context("Unable to collect matching initial load results")?;
        let matching_reserializations = extract_matching_dirs(&regex, &reserialization_dir)?
            .collect::<Result<Vec<_>, _>>()
            .context("Unable to collect matching reserialization results")?;

        for initial_insertion in matching_initial_insertion {
            let file_name = initial_insertion.file_name();
            let file_name = file_name.to_string_lossy();
            let initial_insertion_name = file_name.trim_start_matches(&category);
            let Some(category_pos) = initial_insertion_name.find('[') else {
                anyhow::bail!("Invalid category name {}", initial_insertion_name)
            };
            let backend = &initial_insertion_name[..category_pos - 1];
            let initial_measurement = Measurement {
                reserialization_label: None,
                measure: read_measures(&initial_insertion.path())?,
            };
            let mut reserialization_measurements = matching_reserializations
                .iter()
                .filter(|dir| {
                    let file = dir.file_name();
                    let reser = file.to_string_lossy();

                    reser.starts_with(backend)
                })
                .map(|dir| {
                    Ok(Measurement {
                        reserialization_label: {
                            let name = dir.file_name();
                            let name = name.to_string_lossy();
                            let percent = {
                                let (_, [percent]) = percent_regex
                                    .captures_iter(&name)
                                    .next()
                                    .map(|c| c.extract::<1>())
                                    .expect("");

                                percent
                            };

                            Some(percent.parse().unwrap())
                        },
                        measure: read_measures(&dir.path())?,
                    })
                })
                .collect::<anyhow::Result<Vec<_>>>()?;

            reserialization_measurements.insert(0, initial_measurement);
            measurements
                .entry(Category {
                    map_size,
                    content_size,
                })
                .and_modify(|measurements: &mut Vec<Measurements>| {
                    measurements.push(Measurements {
                        backend: backend.to_owned(),
                        measures: reserialization_measurements.clone(),
                    });
                })
                .or_insert(vec![Measurements {
                    backend: backend.to_owned(),
                    measures: reserialization_measurements,
                }]);
        }
    }

    let rows = 4;
    let cols = measurements.len() / rows;
    let root = SVGBackend::new("measures.svg", (1920, 1200)).into_drawing_area();
    root.fill(&WHITE).unwrap();
    let areas = root.split_evenly((rows, cols));

    for (area_id, (area, (category, measurements))) in
        areas.into_iter().zip(measurements.iter_mut()).enumerate()
    {
        measurements.iter_mut().for_each(|m| {
            m.measures
                .sort_by_key(|m| m.reserialization_label.map(|l| l as i64).unwrap_or(-1));
        });
        let max_value = measurements
            .iter()
            .flat_map(|m| m.measures.iter())
            .max_by(|ma, mb| ma.measure.total_cmp(&mb.measure))
            .unwrap()
            .measure;
        let (divisor, scale) = calculate_scale(max_value);
        let (w, h) = area.dim_in_pixel();
        let (upper, lower) = area.split_vertically((0.90 * h as f64).floor() as u32);
        let mut chart = ChartBuilder::on(&upper)
            .caption(
                format!(
                    "{} map size, {} content size ({})",
                    category.map_size, category.content_size, scale
                ),
                ("sans-serif", 16),
            )
            .x_label_area_size(10)
            .y_label_area_size(0)
            .margin_top(5)
            .margin_bottom(5)
            .margin_left(20)
            .margin_right(20)
            .build_cartesian_2d(0f64..6f64, 0..100)
            .unwrap();

        chart
            .configure_mesh()
            .disable_x_mesh()
            .disable_y_mesh()
            .x_labels(0)
            .draw()
            .unwrap();

        // Draw labels
        let bar_width = 1f64 / measurements.len() as f64;
        let (bar_x0_absolute, _) = chart.plotting_area().map_coordinate(&(0.0, 0));
        let (bar_x1_absolute, _) = chart.plotting_area().map_coordinate(&(bar_width, 0));
        let (cat_x1_absolute, _) = chart.plotting_area().map_coordinate(&(1.0, 0));
        let bar_width_absolute = bar_x1_absolute - bar_x0_absolute;
        let cat_width_absolute = cat_x1_absolute - bar_x0_absolute;
        for (i, cat) in [None, Some(0), Some(25), Some(50), Some(75), Some(100)]
            .into_iter()
            .enumerate()
        {
            let text = match cat {
                None => "Initial",
                Some(0) => "0%",
                Some(25) => "25%",
                Some(50) => "50%",
                Some(75) => "75%",
                Some(100) => "100%",
                _ => "",
            }
            .to_owned();
            let text_style = TextStyle::from(("sans-serif", 11));
            let (text_width, _) = lower.estimate_text_size(&text, &text_style).unwrap();
            let text_width_relative = (text_width as f64) / (cat_width_absolute as f64);
            let text_x0 = (i as f64) + 0.5 - text_width_relative / 2.0;
            let (text_x0, _) = chart.plotting_area().map_coordinate(&(text_x0, -10));
            let row_id = area_id % cols;

            lower
                .draw_text(
                    &text,
                    &text_style,
                    (text_x0 - (row_id as i32) * (w as i32), -10),
                )
                .unwrap();
        }

        // Draw legend
        let backends = ["Postgres", "Fjall"];
        let legend_areas = lower.split_evenly((1, backends.len()));
        for (legend_area, backend) in legend_areas.into_iter().zip(backends) {
            let legend_area = legend_area.margin(0, 0, 30, 30);
            let backend_color = match backend {
                "Postgres" => RED.filled().stroke_width(1),
                "Fjall" => BLUE.filled().stroke_width(1),
                _ => unreachable!("Unhandled backend {}", backend),
            };
            let text_style = TextStyle::from(("sans-serif", 10));
            let x0 = 20;

            legend_area
                .draw(&Rectangle::new([(x0, 3), (x0 + 20, 8)], backend_color))
                .unwrap();
            legend_area
                .draw(&Rectangle::new(
                    [(x0, 3), (x0 + 20, 8)],
                    ShapeStyle {
                        filled: false,
                        color: BLACK.to_rgba(),
                        stroke_width: 1,
                    },
                ))
                .unwrap();
            legend_area
                .draw_text(backend, &text_style, (x0 + 30, 3))
                .unwrap();
        }

        // Draw bars
        for measurement in measurements {
            for measure in measurement.measures.iter() {
                let reserialization_position = match measure.reserialization_label {
                    None => 0f64,
                    Some(0) => 1f64,
                    Some(25) => 2f64,
                    Some(50) => 3f64,
                    Some(75) => 4f64,
                    Some(100) => 5f64,
                    Some(percent) => unreachable!("Unhandled reserialization percent {}", percent),
                };
                let backend_position = if measurement.backend.starts_with("Postgres") {
                    0f64
                } else {
                    1f64
                };
                let backend_color = match &*measurement.backend {
                    "Postgres" => RED.filled(),
                    "Fjall" => BLUE.filled(),
                    _ => unreachable!("Unhandled backend {}", measurement.backend),
                };
                let x0 = backend_position * (bar_width) + reserialization_position;
                let x1 = x0 + bar_width;
                let rect_value = (measure.measure / max_value) * 90f64;
                let mut bar =
                    Rectangle::new([(x0, rect_value.floor() as i32), (x1, 0)], backend_color);
                bar.set_margin(0, 0, 1, 1);
                chart.draw_series([bar]).unwrap();

                let text_value = measure.measure / divisor;
                let text = format_measure(text_value);
                let text_style = TextStyle::from(("sans-serif", 10));
                let (text_width, _) = root.estimate_text_size(&text, &text_style).unwrap();
                let text_width_relative = (text_width as f64) / (bar_width_absolute as f64);
                let text_x0 = x0 + bar_width / 2.0 - text_width_relative / 3.5;
                let text_y0 = (rect_value.floor() as i32) + 4;

                chart
                    .draw_series([Text::new(text, (text_x0, text_y0), text_style)])
                    .unwrap();
            }
        }
    }
    let measurements = measurements
        .into_iter()
        .map(|(k, v)| {
            (
                format!("{} map size {} content size", k.map_size, k.content_size),
                v,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let json_contents = serde_json::to_string(&measurements).unwrap();
    std::fs::write("data.json", json_contents).unwrap();

    Ok(())
}
