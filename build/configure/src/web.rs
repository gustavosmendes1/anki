// Copyright: Ankitects Pty Ltd and contributors
// License: GNU AGPL, version 3 or later; http://www.gnu.org/licenses/agpl.html

use anyhow::Result;
use ninja_gen::action::BuildAction;
use ninja_gen::glob;
use ninja_gen::hashmap;
use ninja_gen::input::BuildInput;
use ninja_gen::inputs;
use ninja_gen::node::node_archive;
use ninja_gen::node::CompileSass;
use ninja_gen::node::CompileTypescript;
use ninja_gen::node::DPrint;
use ninja_gen::node::EsbuildScript;
use ninja_gen::node::Eslint;
use ninja_gen::node::GenTypescriptProto;
use ninja_gen::node::JestTest;
use ninja_gen::node::SqlFormat;
use ninja_gen::node::SvelteCheck;
use ninja_gen::node::TypescriptCheck;
use ninja_gen::rsync::RsyncFiles;
use ninja_gen::Build;

pub fn build_and_check_web(build: &mut Build) -> Result<()> {
    setup_node(build)?;
    build_sass(build)?;
    build_and_check_tslib(build)?;
    declare_and_check_other_libraries(build)?;
    build_and_check_pages(build)?;
    build_and_check_editor(build)?;
    build_and_check_reviewer(build)?;
    build_and_check_mathjax(build)?;
    check_web(build)?;

    Ok(())
}

fn setup_node(build: &mut Build) -> Result<()> {
    ninja_gen::node::setup_node(
        build,
        node_archive(build.host_platform),
        &[
            "dprint",
            "svelte-check",
            "eslint",
            "sass",
            "tsc",
            "tsx",
            "jest",
            "protoc-gen-es",
        ],
        hashmap! {
            "jquery" => vec![
                "jquery/dist/jquery.min.js".into()
            ],
            "jquery-ui" => vec![
                "jquery-ui-dist/jquery-ui.min.js".into()
            ],
            "css-browser-selector" => vec![
                "css-browser-selector/css_browser_selector.min.js".into(),
            ],
            "bootstrap-dist" => vec![
                "bootstrap/dist/js/bootstrap.bundle.min.js".into(),
            ],
            "mathjax" => MATHJAX_FILES.iter().map(|&v| v.into()).collect(),
            "mdi_unthemed" => [
                // saved searches
                "heart-outline.svg",
                // today
                "clock-outline.svg",
                // state
                "circle.svg",
                "circle-outline.svg",
                // flags
                "flag-variant.svg",
                "flag-variant-outline.svg",
                "flag-variant-off-outline.svg",
                // decks
                "book-outline.svg",
                "book-clock-outline.svg",
                "book-cog-outline.svg",
                // notetypes
                "newspaper.svg",
                // cardtype
                "application-braces-outline.svg",
                // fields
                "form-textbox.svg",
                // tags
                "tag-outline.svg",
                "tag-off-outline.svg",
            ].iter().map(|file| format!("@mdi/svg/svg/{file}").into()).collect(),
            "mdi_themed" => [
                // sidebar tools
                "magnify.svg",
                "selection-drag.svg",
                // QComboBox arrows
                "chevron-up.svg",
                "chevron-down.svg",
                // QHeaderView arrows
                "menu-up.svg",
                "menu-down.svg",
                // drag handle
                "drag-vertical.svg",
                "drag-horizontal.svg",
                // checkbox
                "check.svg",
                "minus-thick.svg",
                // QRadioButton
                "circle-medium.svg",
            ].iter().map(|file| format!("@mdi/svg/svg/{file}").into()).collect(),
        },
    )?;
    Ok(())
}

fn build_and_check_tslib(build: &mut Build) -> Result<()> {
    build.add_dependency("ts:lib:i18n", ":rslib:i18n:ts".into());
    build.add_action(
        "ts:lib:proto",
        GenTypescriptProto {
            protos: inputs![glob!["proto/**/*.proto"]],
            include_dirs: &["proto"],
            out_dir: "out/ts/lib",
            out_path_transform: |path| {
                path.replace("proto/", "ts/lib/")
                    .replace("proto\\", "ts/lib\\")
            },
            ts_transform_script: "ts/tools/markpure.ts",
        },
    )?;
    // ensure _service files are generated by rslib
    build.add_dependency("ts:lib:proto", inputs![":rslib:proto:ts"]);
    // the generated _service.js files import @tslib/post, and esbuild won't be able
    // to import the .ts file, so we need to generate a .js file for it
    build.add_action(
        "ts:lib:proto",
        CompileTypescript {
            ts_files: "ts/lib/post.ts".into(),
            out_dir: "out/ts/lib",
            out_path_transform: |path| path.into(),
        },
    )?;

    let src_files = inputs![glob!["ts/lib/**"]];
    eslint(build, "lib", "ts/lib", inputs![":ts:lib", &src_files])?;

    build.add_action(
        "check:jest:lib",
        jest_test("ts/lib", inputs![":ts:lib", &src_files], true),
    )?;

    build.add_dependency("ts:lib", src_files);

    Ok(())
}

fn jest_test(folder: &str, deps: BuildInput, jsdom: bool) -> impl BuildAction + '_ {
    JestTest {
        folder,
        deps,
        jest_rc: "ts/jest.config.js".into(),
        jsdom,
    }
}

fn declare_and_check_other_libraries(build: &mut Build) -> Result<()> {
    for (library, inputs) in [
        ("sveltelib", inputs![":ts:lib", glob!("ts/sveltelib/**")]),
        ("domlib", inputs![":ts:lib", glob!("ts/domlib/**")]),
        (
            "components",
            inputs![":ts:lib", ":ts:sveltelib", glob!("ts/components/**")],
        ),
        ("html-filter", inputs![glob!("ts/html-filter/**")]),
    ] {
        let library_with_ts = format!("ts:{library}");
        let folder = library_with_ts.replace(':', "/");
        build.add_dependency(&library_with_ts, inputs.clone());
        eslint(build, library, &folder, inputs.clone())?;

        if matches!(library, "domlib" | "html-filter") {
            build.add_action(
                &format!("check:jest:{library}"),
                jest_test(&folder, inputs, true),
            )?;
        }
    }

    eslint(build, "scripts", "ts/tools", inputs![glob!("ts/tools/*")])?;

    Ok(())
}

pub fn eslint(build: &mut Build, name: &str, folder: &str, deps: BuildInput) -> Result<()> {
    let eslint_rc = inputs![".eslintrc.js"];
    build.add_action(
        format!("check:eslint:{name}"),
        Eslint {
            folder,
            inputs: deps.clone(),
            eslint_rc: eslint_rc.clone(),
            fix: false,
        },
    )?;
    build.add_action(
        format!("fix:eslint:{name}"),
        Eslint {
            folder,
            inputs: deps,
            eslint_rc,
            fix: true,
        },
    )?;
    Ok(())
}

fn build_and_check_pages(build: &mut Build) -> Result<()> {
    build.add_dependency("ts:tag-editor", inputs![glob!["ts/tag-editor/**"]]);

    let mut build_page = |name: &str, html: bool, deps: BuildInput| -> Result<()> {
        let group = format!("ts:{name}");
        let deps = inputs![deps, glob!(format!("ts/{name}/**"))];
        let extra_exts = if html { &["css", "html"][..] } else { &["css"] };
        build.add_action(
            &group,
            EsbuildScript {
                script: inputs!["ts/bundle_svelte.mjs"],
                entrypoint: inputs![format!("ts/{name}/index.ts")],
                output_stem: &format!("ts/{name}/{name}"),
                deps: deps.clone(),
                extra_exts,
            },
        )?;
        build.add_dependency("ts:pages", inputs![format!(":{group}")]);
        build.add_action(
            format!("check:svelte:{name}"),
            SvelteCheck {
                tsconfig: inputs![format!("ts/{name}/tsconfig.json")],
                inputs: deps.clone(),
            },
        )?;
        let folder = format!("ts/{name}");
        eslint(build, name, &folder, deps.clone())?;
        if matches!(name, "deck-options" | "change-notetype") {
            build.add_action(
                &format!("check:jest:{name}"),
                jest_test(&folder, deps, false),
            )?;
        }

        Ok(())
    };
    build_page(
        "congrats",
        true,
        inputs![
            //
            ":ts:lib",
            ":ts:components",
            ":sass",
        ],
    )?;
    build_page(
        "deck-options",
        true,
        inputs![
            //
            ":ts:lib",
            ":ts:components",
            ":ts:sveltelib",
            ":sass",
        ],
    )?;
    build_page(
        "graphs",
        true,
        inputs![
            //
            ":ts:lib",
            ":ts:components",
            ":sass",
        ],
    )?;
    build_page(
        "card-info",
        true,
        inputs![
            //
            ":ts:lib",
            ":ts:components",
            ":sass",
        ],
    )?;
    build_page(
        "change-notetype",
        true,
        inputs![
            //
            ":ts:lib",
            ":ts:components",
            ":ts:sveltelib",
            ":sass",
        ],
    )?;
    build_page(
        "import-csv",
        true,
        inputs![
            //
            ":ts:lib",
            ":ts:components",
            ":ts:sveltelib",
            ":ts:tag-editor",
            ":sass"
        ],
    )?;
    build_page(
        "import-anki-package",
        true,
        inputs![
            //
            ":ts:lib",
            ":ts:components",
            ":ts:sveltelib",
            ":sass"
        ],
    )?;
    // we use the generated .css file separately
    build_page(
        "editable",
        false,
        inputs![
            //
            ":ts:lib",
            ":ts:components",
            ":ts:domlib",
            ":ts:sveltelib",
            ":sass"
        ],
    )?;
    build_page(
        "image-occlusion",
        true,
        inputs![
            //
            ":ts:lib",
            ":ts:components",
            ":ts:sveltelib",
            ":ts:tag-editor",
            ":sass"
        ],
    )?;
    build_page(
        "import-page",
        true,
        inputs![
            //
            ":ts:lib",
            ":ts:components",
            ":ts:sveltelib",
            ":sass"
        ],
    )?;

    Ok(())
}

fn build_and_check_editor(build: &mut Build) -> Result<()> {
    let editor_deps = inputs![
        //
        ":ts:lib",
        ":ts:components",
        ":ts:domlib",
        ":ts:sveltelib",
        ":ts:tag-editor",
        ":ts:html-filter",
        ":ts:image-occlusion",
        ":sass",
        glob!("ts/{editable,editor}/**")
    ];

    build.add_action(
        "ts:editor",
        EsbuildScript {
            script: "ts/bundle_svelte.mjs".into(),
            entrypoint: "ts/editor/index.ts".into(),
            output_stem: "ts/editor/editor",
            deps: editor_deps.clone(),
            extra_exts: &["css"],
        },
    )?;

    let group = "ts/editor";
    build.add_action(
        "check:svelte:editor",
        SvelteCheck {
            tsconfig: inputs![format!("{group}/tsconfig.json")],
            inputs: editor_deps.clone(),
        },
    )?;
    eslint(build, "editor", group, editor_deps)?;
    Ok(())
}

fn build_and_check_reviewer(build: &mut Build) -> Result<()> {
    let reviewer_deps = inputs![":ts:lib", glob!("ts/{reviewer,image-occlusion}/**"),];
    build.add_action(
        "ts:reviewer:reviewer.js",
        EsbuildScript {
            script: inputs!["ts/bundle_ts.mjs"],
            entrypoint: "ts/reviewer/index_wrapper.ts".into(),
            output_stem: "ts/reviewer/reviewer",
            deps: reviewer_deps.clone(),
            extra_exts: &[],
        },
    )?;
    build.add_action(
        "ts:reviewer:reviewer.css",
        CompileSass {
            input: inputs!["ts/reviewer/reviewer.scss"],
            output: "ts/reviewer/reviewer.css",
            deps: inputs![":sass", "ts/image-occlusion/review.scss"],
            load_paths: vec!["."],
        },
    )?;
    build.add_action(
        "ts:reviewer:reviewer_extras_bundle.js",
        EsbuildScript {
            script: inputs!["ts/bundle_ts.mjs"],
            entrypoint: "ts/reviewer/reviewer_extras.ts".into(),
            output_stem: "ts/reviewer/reviewer_extras_bundle",
            deps: reviewer_deps.clone(),
            extra_exts: &[],
        },
    )?;
    build.add_action(
        "ts:reviewer:reviewer_extras.css",
        CompileSass {
            input: inputs!["ts/reviewer/reviewer_extras.scss"],
            output: "ts/reviewer/reviewer_extras.css",
            deps: inputs!["ts/image-occlusion/review.scss"],
            load_paths: vec!["."],
        },
    )?;

    build.add_action(
        "check:typescript:reviewer",
        TypescriptCheck {
            tsconfig: inputs!["ts/reviewer/tsconfig.json"],
            inputs: reviewer_deps.clone(),
        },
    )?;
    eslint(build, "reviewer", "ts/reviewer", reviewer_deps)?;
    build.add_action(
        "check:jest:reviewer",
        jest_test("ts/reviewer", inputs![":ts:reviewer"], false),
    )?;
    Ok(())
}

fn check_web(build: &mut Build) -> Result<()> {
    let dprint_files = inputs![glob![
        "**/*.{ts,mjs,js,md,json,toml,svelte,scss}",
        "target/**"
    ]];
    build.add_action(
        "check:format:dprint",
        DPrint {
            inputs: dprint_files.clone(),
            check_only: true,
        },
    )?;
    build.add_action(
        "format:dprint",
        DPrint {
            inputs: dprint_files,
            check_only: false,
        },
    )?;

    Ok(())
}

pub fn check_sql(build: &mut Build) -> Result<()> {
    build.add_action(
        "check:format:sql",
        SqlFormat {
            inputs: inputs![glob!["**/*.sql"]],
            check_only: true,
        },
    )?;
    build.add_action(
        "format:sql",
        SqlFormat {
            inputs: inputs![glob!["**/*.sql"]],
            check_only: false,
        },
    )?;
    Ok(())
}

fn build_and_check_mathjax(build: &mut Build) -> Result<()> {
    let files = inputs![glob!["ts/mathjax/*"]];
    build.add_action(
        "ts:mathjax",
        EsbuildScript {
            script: "ts/transform_ts.mjs".into(),
            entrypoint: "ts/mathjax/index.ts".into(),
            deps: files.clone(),
            output_stem: "ts/mathjax/mathjax",
            extra_exts: &[],
        },
    )?;
    eslint(build, "mathjax", "ts/mathjax", files.clone())?;
    build.add_action(
        "check:typescript:mathjax",
        TypescriptCheck {
            tsconfig: "ts/mathjax/tsconfig.json".into(),
            inputs: files,
        },
    )
}

pub const MATHJAX_FILES: &[&str] = &[
    "mathjax/es5/a11y/assistive-mml.js",
    "mathjax/es5/a11y/complexity.js",
    "mathjax/es5/a11y/explorer.js",
    "mathjax/es5/a11y/semantic-enrich.js",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_AMS-Regular.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Calligraphic-Bold.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Calligraphic-Regular.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Fraktur-Bold.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Fraktur-Regular.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Main-Bold.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Main-Italic.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Main-Regular.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Math-BoldItalic.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Math-Italic.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Math-Regular.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_SansSerif-Bold.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_SansSerif-Italic.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_SansSerif-Regular.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Script-Regular.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Size1-Regular.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Size2-Regular.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Size3-Regular.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Size4-Regular.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Typewriter-Regular.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Vector-Bold.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Vector-Regular.woff",
    "mathjax/es5/output/chtml/fonts/woff-v2/MathJax_Zero.woff",
    "mathjax/es5/tex-chtml-full.js",
    "mathjax/es5/sre/mathmaps/de.json",
    "mathjax/es5/sre/mathmaps/en.json",
    "mathjax/es5/sre/mathmaps/es.json",
    "mathjax/es5/sre/mathmaps/fr.json",
    "mathjax/es5/sre/mathmaps/hi.json",
    "mathjax/es5/sre/mathmaps/it.json",
    "mathjax/es5/sre/mathmaps/nemeth.json",
];

pub fn copy_mathjax() -> impl BuildAction {
    RsyncFiles {
        inputs: inputs![":node_modules:mathjax"],
        target_folder: "qt/_aqt/data/web/js/vendor/mathjax",
        strip_prefix: "$builddir/node_modules/mathjax/es5",
        extra_args: "",
    }
}

fn build_sass(build: &mut Build) -> Result<()> {
    build.add_dependency("sass", inputs![glob!("sass/**")]);

    build.add_action(
        "css:_root-vars",
        CompileSass {
            input: inputs!["sass/_root-vars.scss"],
            output: "sass/_root-vars.css",
            deps: inputs![glob!["sass/*"]],
            load_paths: vec![],
        },
    )?;

    Ok(())
}
