#!/usr/bin/env python3
"""Qualify bounded raster changes by replaying captured geometry in both sources.

Uses the frozen old renderer and the actual current renderer, checks each
triangle's coverage against the independent closed-form edge diagnostic, and
requires changed native bitmap/palette cells to contain a changed full-color
scene pixel. Geometry/game changes fail explicitly; no hash mismatch is waived.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def load_comparison():
    spec = importlib.util.spec_from_file_location("benchmark_comparison", ROOT / "tools/compare-benchmarks.py")
    comparison = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(comparison)
    return comparison


REPLAY = r'''
pub fn replay(input:&[u8],clear_color:u8)->Vec<u8> {
    assert_eq!(input.len()%16,0);
    let count=input.len()/16;
    assert!(count<=TRIANGLE_CAPACITY);
    let mut renderer=Renderer::new();
    renderer.begin(clear_color);
    renderer.triangle_count=count as u16;
    for (index,bytes) in input.chunks_exact(16).enumerate() {
        let word=|at|i16::from_le_bytes([bytes[at],bytes[at+1]]);
        renderer.triangles[index]=Triangle{x:[word(0),word(2),word(4)],y:[word(6),word(8),word(10)],depth:word(12),color:bytes[14],reserved:bytes[15]};
    }
    let commands=renderer.triangles[..count].to_vec();
    let mut output=painted_frame(&mut renderer);
    // Repaint each triangle with a nonzero mask color, preserving its exact
    // captured coordinates and metadata. Record actual covered row endpoints.
    let mut single=Renderer::new();
    for index in 0..count {
        single.begin(0);single.triangle_count=1;
        single.triangles[0]=commands[index];single.triangles[0].color=1;
        let pixels=painted_frame(&mut single);
        for row in pixels.chunks_exact(160) {
                let (left,right)=match row.iter().position(|&color|color!=0) {
                    Some(left)=>{
                        let right=row.iter().rposition(|&color|color!=0).unwrap()+1;
                        assert!(row[left..right].iter().all(|&color|color==1),"noncontiguous triangle span");
                        (left as u16,right as u16)
                    },
                    None=>(u16::MAX,u16::MAX),
                };
                output.extend_from_slice(&left.to_le_bytes());output.extend_from_slice(&right.to_le_bytes());
        }
    }
    output
}
'''


def compile_renderer(source_root, build, name):
    source_root = source_root.resolve()
    render = (source_root / "src/render.rs").read_text()
    copied = "\n".join(line for line in render.splitlines() if not line.startswith("//!"))
    copied = copied.replace("mod sort;", f'#[path="{source_root / "src/render/sort.rs"}"] mod sort;')
    traversal = ("let mut pass=renderer.raster_pass().unwrap(); for band in 0..25 { "
                 "assert_eq!(pass.next_band(&mut pixels,true),Some(band)); output.extend_from_slice(&pixels); }") if name == "new" else (
                 "for band in 0..25 { renderer.render_band(band,&mut pixels); output.extend_from_slice(&pixels); }")
    source = f'''#![allow(dead_code,unused_imports)]
#[derive(Clone,Copy,Default,Debug,PartialEq,Eq)] pub struct Vec3 {{ pub x:i16,pub y:i16,pub z:i16 }}
pub const TWO_SIDED:u8=1;
pub trait GeometrySink {{ fn triangle(&mut self,a:Vec3,b:Vec3,c:Vec3,color:u8,flags:u8); }}
mod game {{ pub(crate) const STATIC_SKY:u8=2; }}
#[path="{source_root / 'src/math.rs'}"] mod math;
#[path="{source_root / 'src/projection.rs'}"] mod projection;
mod render {{
{copied}
{REPLAY}
fn painted_frame(renderer:&mut Renderer)->Vec<u8> {{
    let mut output=Vec::with_capacity(32000);let mut pixels=[0;1280];
    {traversal}
    output
}}
}}
fn main() {{
 let args:Vec<_>=std::env::args().collect();
 let input=std::fs::read(&args[1]).unwrap();
 std::fs::write(&args[2],render::replay(&input,args[3].parse().unwrap())).unwrap();
}}
'''
    path = build / f"{name}.rs"
    path.write_text(source)
    executable = build / name
    subprocess.run(["rustc", "--edition=2021", "-O", path, "-o", executable], check=True)
    return executable, {"render": hashlib.sha256(render.encode()).hexdigest(),
                        "math": hashlib.sha256((source_root / "src/math.rs").read_bytes()).hexdigest()}


def capture_path(sample):
    return Path(sample["capture"]["native"])


def replay(executable, sample, output):
    geometry = json.loads(capture_path(sample).with_name("geometry.json").read_text())
    commands = b"".join(struct.pack("<7h2B", *triangle["x"], *triangle["y"], triangle["depth"],
                                   triangle["color"], triangle["reserved"]) for triangle in geometry)
    inputs = output.with_suffix(".commands")
    inputs.write_bytes(commands)
    subprocess.run([executable, inputs, output, str(sample.get("clear_color", 6))], check=True)
    result = output.read_bytes()
    if len(result) != 32000 + len(geometry) * 800:
        raise RuntimeError("unexpected replay length")
    return geometry, result[:32000], list(struct.iter_unpack("<HH", result[32000:]))


def semantic_geometry(geometry):
    return [dict(triangle, reserved=triangle["reserved"] & ~6) for triangle in geometry]


def span(left, right):
    return set() if left == 65535 else set(range(left, right))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("candidate", type=Path)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--baseline-source", type=Path, default=ROOT / "target/bench/baseline-source")
    args = parser.parse_args()
    args.out_dir.mkdir(parents=True, exist_ok=True)
    build = args.out_dir / "replay"
    build.mkdir(exist_ok=True)
    comparison = load_comparison()
    baseline, candidate = [json.loads(path.read_text()) for path in (args.baseline, args.candidate)]
    if baseline["video"] != candidate["video"]:
        parser.error("video standards differ")
    old, old_hash = compile_renderer(args.baseline_source, build, "old")
    new, new_hash = compile_renderer(ROOT, build, "new")
    reference = {(case["fixture"], case["ticks"], case["palette"]): case for case in baseline["cases"]}
    report = {"baseline": str(args.baseline), "candidate": str(args.candidate), "video": candidate["video"],
              "scope": "Actual old/new Rust replay, independent per-triangle <=1px edge gate, affected native cells only",
              "source_sha256": {"baseline": old_hash, "candidate": new_hash}, "cases": []}
    for case in candidate["cases"]:
        key = case["fixture"], case["ticks"], case["palette"]
        name = "-".join(map(str, key))
        result = {"fixture": key[0], "ticks": key[1], "palette": key[2], "passed": False}
        report["cases"].append(result)
        if key not in reference or not case["samples"] or not reference[key]["samples"]:
            result["error"] = "missing baseline/candidate sample"
            continue
        a, b = reference[key]["samples"][0], case["samples"][0]
        ga, scene_a, spans_a = replay(old, a, build / f"{name}-old.bin")
        gb, scene_b, spans_b = replay(new, b, build / f"{name}-new.bin")
        sa, sb = semantic_geometry(ga), semantic_geometry(gb)
        result.update(game_exact=a["game_sha256"] == b["game_sha256"], geometry_exact=sa == sb,
                      frame_serial_exact=a["frame_serial"] == b["frame_serial"], triangle_counts=[len(ga), len(gb)])
        if sa != sb:
            result["geometry_differences"] = [{"index": i, "baseline": sa[i] if i < len(sa) else None,
                                                "candidate": sb[i] if i < len(sb) else None}
                                               for i in range(max(len(sa), len(sb)))
                                               if i >= len(sa) or i >= len(sb) or sa[i] != sb[i]]
            continue
        coverage_errors, maximum = [], 0
        for index, triangle in enumerate(sa):
            allowed, rounding = comparison.row_rounding([triangle])
            maximum = max(maximum, rounding["maximum_endpoint_delta"])
            coverage_errors.extend(dict(error, triangle=index) for error in rounding["invalid_rows"])
            actual_changes = set()
            for row in range(200):
                original, updated = spans_a[index * 200 + row], spans_b[index * 200 + row]
                actual_changes.update((x, row) for x in span(*original) ^ span(*updated))
            if actual_changes != allowed:
                coverage_errors.append({"triangle": index, "host_only_pixels": sorted(actual_changes - allowed)[:32],
                                        "formula_only_pixels": sorted(allowed - actual_changes)[:32]})
        scene_changed = [index for index, colors in enumerate(zip(scene_a, scene_b)) if colors[0] != colors[1]]
        affected = {index // 160 // 8 * 40 + index % 160 // 4 for index in scene_changed}
        native_a, native_b = capture_path(a).read_bytes(), capture_path(b).read_bytes()
        if len(native_a) != 10000 or len(native_b) != 10000:
            raise RuntimeError("native capture must contain8000bitmap+1000screen+1000color bytes")
        native_changed = {cell for cell in range(1000) if
                          native_a[cell*8:cell*8+8] != native_b[cell*8:cell*8+8]
                          or native_a[8000+cell] != native_b[8000+cell]
                          or native_a[9000+cell] != native_b[9000+cell]}
        outside = native_changed - affected
        result.update(maximum_endpoint_delta=maximum, coverage_errors=coverage_errors,
                      scene_changed_pixels=len(scene_changed), affected_cells=sorted(affected),
                      native_changed_cells=sorted(native_changed), unexplained_native_cells=sorted(outside),
                      native_exact=native_a == native_b)
        result["passed"] = (result["game_exact"] and result["geometry_exact"] and result["frame_serial_exact"]
                            and not coverage_errors and not outside and maximum <= 1)
        print(f"{name}: {'PASS' if result['passed'] else 'FAIL'}; triangles={len(sa)}, maxedge={maximum}, "
              f"scene={len(scene_changed)}px/{len(affected)}cells, native={len(native_changed)}cells, unexplained={len(outside)}", flush=True)
    report["passed"] = bool(report["cases"]) and all(case["passed"] for case in report["cases"])
    report["case_count"] = len(report["cases"])
    (args.out_dir / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
