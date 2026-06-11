//! Round-trip table for every recognised shorthand → canonical
//! `generate://` URI translation.

use oxideav_generator::shorthand::translate;

#[test]
fn all_shorthand_translations_round_trip() {
    let cases = [
        ("xc:red", "generate://xc?color=red"),
        ("xc:#ff0000", "generate://xc?color=%23ff0000"),
        (
            "pattern:checkerboard",
            "generate://pattern?type=checkerboard",
        ),
        ("gradient:red-blue", "generate://gradient?from=red&to=blue"),
        (
            "radial:red-blue",
            "generate://gradient?type=radial&from=red&to=blue",
        ),
        ("plasma:", "generate://plasma"),
        ("mandelbrot:", "generate://fractal?type=mandelbrot"),
        ("julia:", "generate://fractal?type=julia"),
        (
            "synth:5,sine,440",
            "generate://synth?duration=5&type=sine&freq=440",
        ),
        (
            "synth:10,pluck,440",
            "generate://synth?duration=10&type=pluck&freq=440",
        ),
        ("testsrc:", "generate://testsrc"),
        ("smptebars:", "generate://smptebars"),
        ("zoneplate:", "generate://zoneplate"),
        ("scroll:", "generate://scroll"),
        (
            "scroll:pattern=plasma&vx=2&vy=-1",
            "generate://scroll?pattern=plasma&vx=2&vy=-1",
        ),
        ("noise:perlin", "generate://noise?type=perlin"),
    ];
    for (input, want) in cases {
        let got = translate(input);
        assert_eq!(got, want, "shorthand for {input:?}");
    }
}

#[test]
fn passthrough_for_unrecognised_inputs() {
    let pass = [
        "in.png",
        "out.wav",
        "file:///tmp/x.mp4",
        "https://example.com/a.flac",
        "/absolute/path",
        "generate://synth?type=sine",
    ];
    for p in pass {
        assert_eq!(translate(p), p);
    }
}
