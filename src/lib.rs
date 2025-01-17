#[macro_use]
extern crate log;
#[macro_use]
extern crate lazy_static;
mod ansi;
mod engine;
mod event;
mod field;
mod header;
mod input;
mod item;
mod matcher;
mod model;
mod options;
mod orderedvec;
mod output;
mod previewer;
mod query;
mod reader;
mod score;
mod selection;
mod spinlock;
mod theme;
mod util;

use crate::event::Event::*;
use crate::event::{EventReceiver, EventSender};
use crate::model::Model;
pub use crate::options::{SkimOptions, SkimOptionsBuilder};
pub use crate::output::SkimOutput;
use crate::reader::Reader;
use nix::unistd::isatty;
use std::env;
use std::io::BufRead;
use std::io::BufReader;
use std::os::unix::io::AsRawFd;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread;
use tuikit::prelude::{Event as TermEvent, *};

pub struct Skim {}

impl Skim {
    // TODO main関数から呼ばれるときsourceはNone
    pub fn run_with(options: &SkimOptions, source: Option<Box<dyn BufRead + Send>>) -> Option<SkimOutput> {
        let min_height = options
            .min_height
            .map(Skim::parse_height_string)
            .expect("min_height should have default values");
        let height = options
            .height
            .map(Skim::parse_height_string)
            .expect("height should have default values");

        // std::sync系パッケージを使った処理
        let (tx, rx): (EventSender, EventReceiver) = channel(); // なぜ型注釈をつけているのか？ここは型推論で問題なさそう

        // Arc型
        let term = Arc::new(Term::with_options(TermOptions::default().min_height(min_height).height(height)).unwrap());

        //------------------------------------------------------------------------------
        // input
        let mut input = input::Input::new(); // optionからえられた値(ユーザーの入力値) = input
        input.parse_keymaps(&options.bind); // ユーザー入力値を代入
        input.parse_expect_keys(options.expect.as_ref().map(|x| &**x)); // ユーザー入力値を代入

        let tx_clone = tx.clone(); // 送信側を増やす
        let term_clone = term.clone(); // pointerを取得

        // 非同期処理
        let input_thread = thread::spawn(move || 'outer: loop {
            if let Ok(key) = term_clone.poll_event() {
                if key == TermEvent::User1 {
                    // thread::sleep_ms(5000);
                    break; // model.start()の後にsendされている
                }

                // tx.sendで(event, 何か?)を送信している
                // key入力をevent.rsの型に翻訳
                for (ev, arg) in input.translate_event(key).into_iter() {
                    // println!("{:?}, {:?}", ev, arg);
                    let _ = tx_clone.send((ev, arg));
                }
            }
        });

        let reader = Reader::with_options(&options); // sourceを削除

        //------------------------------------------------------------------------------
        // start a timer for notifying refresh(リフレッシュを通知するためのtimerを開始する)
        // 上記同様送信している
        let _ = tx.send((EvHeartBeat, Box::new(true))); // これがないと結果が出力されない

        //------------------------------------------------------------------------------
        // model + previewer
        // Model::newはコンフィグ設定で非常に重要な役割を担っている
        // この前の処理でSkimOptionにParseした値をnew関数に代入することで
        // queryやmatcherという各責務を持つコンポーネントのコンフィグクラスに設定を入れ直す
        // 例えばoption.cmdはquery.base_cmdに入れ直される
        // そのqueryはModel.queryに設定される
        let mut model = Model::new(rx, tx, reader, term.clone(), &options);
        let ret = model.start(); // イベントレシーバ処理の根幹がここっぽい

        // println!("model.start()が無限ループ");
        // thread::sleep_ms(5000);
        let _ = term.send_event(TermEvent::User1); // interrupt the input thread
        let _ = input_thread.join(); // スレッド終了まで待機
        let _ = term.pause();
        ret
    }

    pub fn filter(options: &SkimOptions, source: Option<Box<dyn BufRead + Send>>) -> i32 {
        use crate::engine::{EngineFactory, MatcherMode};

        let output_ending = if options.print0 { "\0" } else { "\n" };
        let query = options.filter;
        let default_command = match env::var("SKIM_DEFAULT_COMMAND").as_ref().map(String::as_ref) {
            Ok("") | Err(_) => "find .".to_owned(),
            Ok(val) => val.to_owned(),
        };

        let cmd = options.cmd.unwrap_or(&default_command);

        // output query
        if options.print_query {
            print!("{}{}", query, output_ending);
        }

        if options.print_cmd {
            print!("{}{}", cmd, output_ending);
        }

        //------------------------------------------------------------------------------
        // reader

        // in piped situation(e.g. `echo "a" | sk`) set source to the pipe
        let source = source.or_else(|| {
            let stdin = std::io::stdin();
            if !isatty(stdin.as_raw_fd()).unwrap_or(true) {
                Some(Box::new(BufReader::new(stdin)))
            } else {
                None
            }
        });

        let mut reader = Reader::with_options(&options).source(source);

        //------------------------------------------------------------------------------
        // matcher
        let matcher_mode = if options.regex {
            MatcherMode::Regex
        } else if options.exact {
            MatcherMode::Exact
        } else {
            MatcherMode::Fuzzy
        };

        let engine = EngineFactory::build(query, matcher_mode);

        //------------------------------------------------------------------------------
        // start
        let reader_control = reader.run(cmd);

        let mut match_count = 0;
        while !reader_control.is_done() {
            for item in reader_control.take().into_iter() {
                if let Some(matched) = engine.match_item(item) {
                    println!("{}\t{}", -matched.rank.score, matched.item.get_output_text());
                    match_count += 1;
                }
            }
        }

        if match_count == 0 {
            return 1;
        } else {
            return 0;
        }
    }

    // 10 -> TermHeight::Fixed(10)
    // 10% -> TermHeight::Percent(10)
    fn parse_height_string(string: &str) -> TermHeight {
        if string.ends_with('%') {
            TermHeight::Percent(string[0..string.len() - 1].parse().unwrap_or(100))
        } else {
            TermHeight::Fixed(string.parse().unwrap_or(0))
        }
    }
}
