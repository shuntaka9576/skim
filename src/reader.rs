///! Reader is used for reading items from datasource (e.g. stdin or command output)
///!
///! After reading in a line, reader will save an item into the pool(items)
use crate::field::FieldRange;
use crate::item::Item;
use crate::options::SkimOptions;
use crate::spinlock::SpinLock;
use regex::Regex;
use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

const DELIMITER_STR: &str = r"[\t\n ]+";

#[derive(Debug)]
pub struct ReaderControl {
    stopped: Arc<AtomicBool>,
    thread_reader: JoinHandle<()>,
    pub items: Arc<SpinLock<Vec<Arc<Item>>>>,
}

impl ReaderControl {
    pub fn kill(self) {
        self.stopped.store(true, Ordering::SeqCst);
        let _ = self.thread_reader.join();
    }

    pub fn take(&self) -> Vec<Arc<Item>> {
        let mut items = self.items.lock();
        let mut ret = Vec::with_capacity(items.len());
        ret.append(&mut items);
        ret
    }

    pub fn is_done(&self) -> bool {
        let items = self.items.lock();
        self.stopped.load(Ordering::Relaxed) && items.is_empty()
    }
}

pub struct Reader {
    option: Arc<ReaderOption>,
    source_file: Option<Box<dyn BufRead + Send>>,
}

impl Reader {
    pub fn with_options(options: &SkimOptions) -> Self {
        Self {
            option: Arc::new(ReaderOption::with_options(&options)),
            source_file: None,
        }
    }

    pub fn source(mut self, source_file: Option<Box<dyn BufRead + Send>>) -> Self {
        self.source_file = source_file;
        self
    }

    // -c オプションのコマンド実行してそうなところ
    // 引数にコマンドを入れると実行してくレル関数
    // TODO Readerの初期化状態を確認すべき
    pub fn run(&mut self, cmd: &str) -> ReaderControl {
        let stopped = Arc::new(AtomicBool::new(false));
        let stopped_clone = stopped.clone();
        stopped.store(false, Ordering::SeqCst); // 初回はfalse, Ordering::SeqCstはよくわからない。スレッドの順序がどうこう..

        let items = Arc::new(SpinLock::new(Vec::new()));
        let items_clone = items.clone();

        let option_clone = self.option.clone(); // 多分Model::newで初期化されたことをクローンしている?
        let source_file = self.source_file.take(); // Noneで初期化されている
        let cmd = cmd.to_string();

        // start the new command
        // コマンド実行は別スレッドされるが、コマンド結果を受け取るまでこのスレッドはブロックする
        let thread_reader = thread::spawn(move || {
            reader(&cmd, stopped_clone, items_clone, option_clone, source_file);
        });

        ReaderControl {
            stopped,       // AtomicBool(コマンドの実行の終了を渡す)
            thread_reader, // 実行結果を渡す
            items,
        }
    }
}

struct ReaderOption {
    pub use_ansi_color: bool,
    pub default_arg: String,
    pub transform_fields: Vec<FieldRange>,
    pub matching_fields: Vec<FieldRange>,
    pub delimiter: Regex,
    pub replace_str: String,
    pub line_ending: u8,
}

impl ReaderOption {
    pub fn new() -> Self {
        ReaderOption {
            use_ansi_color: false,
            default_arg: String::new(),
            transform_fields: Vec::new(),
            matching_fields: Vec::new(),
            delimiter: Regex::new(DELIMITER_STR).unwrap(),
            replace_str: "{}".to_string(),
            line_ending: b'\n',
        }
    }

    pub fn with_options(options: &SkimOptions) -> Self {
        let mut reader_option = Self::new();
        reader_option.parse_options(&options);
        reader_option
    }

    fn parse_options(&mut self, options: &SkimOptions) {
        if options.ansi {
            self.use_ansi_color = true;
        }

        if let Some(delimiter) = options.delimiter {
            self.delimiter = Regex::new(delimiter).unwrap_or_else(|_| Regex::new(DELIMITER_STR).unwrap());
        }

        if let Some(transform_fields) = options.with_nth {
            self.transform_fields = transform_fields
                .split(',')
                .filter_map(|string| FieldRange::from_str(string))
                .collect();
        }

        if let Some(matching_fields) = options.nth {
            self.matching_fields = matching_fields
                .split(',')
                .filter_map(|string| FieldRange::from_str(string))
                .collect();
        }

        if options.read0 {
            self.line_ending = b'\0';
        }
    }
}
// Sendでスレッド間で送信可能になる
type CommandOutput = (Option<Child>, Box<dyn BufRead + Send>);

// 引数に与えられたコマンドを元に、子プロセスを実行
// 子プロセスを表すchildとsの標準出力stdoutを返却
// Result<(CommandOutput, Box<dyn Error>)とは書かない? -> 勘違い
// Reuslt<T,E>でOk(T)でErr(E)が返却
// CommandOputputってErrorをトレイトオブジェクトで返す
fn get_command_output(cmd: &str) -> Result<CommandOutput, Box<dyn Error>> {
    let shell = env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
    println!("{}", shell);
    let mut command = Command::new(shell)
        .arg("-c")
        .arg(cmd)
        .stdout(Stdio::piped()) // 標準出力はパイプに書く
        .stderr(Stdio::null()) // 標準エラー出力は/dev/nullに書く
        .spawn()?; // 失敗したら、Errorがこの時点で返却される Reuslt<Child>

    let stdout = command
        .stdout // Option<ChildStdout>
        .take() // 値がある場合はSome(T)、ない場合はNone。panicが起きないので、safeなメソッド
        .ok_or_else(|| "command output: unwrap failed".to_owned())?; // ChildStdout

    // commandはchild
    // BufRead::newはRead Traitがついて入れば引数に取れる
    // ChildStdoutはReadTraitが付いてているのOK
    Ok((Some(command), Box::new(BufReader::new(stdout))))
}

// Consider that you invoke a command with different arguments several times
// If you select some items each time, how will skim remeber it?
// => Well, we'll give each invokation a number, i.e. RUN_NUM
// What if you invoke the same command and same arguments twice?
// => We use NUM_MAP to specify the same run number.
lazy_static! {
    static ref RUN_NUM: RwLock<usize> = RwLock::new(0);
    static ref NUM_MAP: RwLock<HashMap<String, usize>> = RwLock::new(HashMap::new());
}

// reader.runでは別スレッド上で実行されている
fn reader(
    cmd: &str,
    stopped: Arc<AtomicBool>,
    items: Arc<SpinLock<Vec<Arc<Item>>>>,
    option: Arc<ReaderOption>,
    source_file: Option<Box<dyn BufRead + Send>>,
) {
    // command実行箇所?
    // コマンド実行はブロックする
    let (command, mut source) = source_file // Some(ChildStdout), Box::new(BufReader::new(stdout))
        .map(|f| (None, f)) // NoneとOption<Box..型を返す(sourceがあるなら、それを返せよという意味)
        // get_command_outputでOk((Some(command<Child>), Box::new(BufReader::new(stdout))))が返却
        .unwrap_or_else(|| get_command_output(cmd).expect("command not found"));

    let command_stopped = Arc::new(AtomicBool::new(false));

    let stopped_clone = stopped.clone(); // stopped(false)
    let command_stopped_clone = command_stopped.clone(); // command_stopped(false)
    thread::spawn(move || {
        // kill command if it is got
        // 起動直後はこのループが周り続けそう(stopped_cloneの値がどこかでtrueになったら終わる)
        // stopped_cloneは、おそらく-cオプションで指定したコマンド終了時にtrueになる?
        // 誰がstopped_cloneを書き換えているのか?
        while command.is_some() && !stopped_clone.load(Ordering::Relaxed) {
            // println!("{}", stopped_clone.load(Ordering::Relaxed));
            thread::sleep(Duration::from_millis(5));
        }

        // clean up resources
        // Optionの中身のChildを操作している
        if let Some(mut x) = command {
            let _ = x.kill();
            let _ = x.wait();
        }
        command_stopped_clone.store(true, Ordering::Relaxed);
    });

    let opt = option;

    // set the proper run number
    let run_num = { *RUN_NUM.read().expect("reader: failed to lock RUN_NUM") };
    let run_num = *NUM_MAP
        .write()
        .expect("reader: failed to lock NUM_MAP")
        .entry(cmd.to_string())
        .or_insert_with(|| {
            *(RUN_NUM.write().expect("reader: failed to lock RUN_NUM for write")) = run_num + 1;
            run_num + 1
        });

    let mut index = 0;
    let mut buffer = Vec::with_capacity(100);
    loop {
        buffer.clear();
        // start reading
        // line_endingはデフォルト b'\n'
        // コマンド実行結果のうち改行を含むまでの数値がnに入る
        // 改行までの値は、bufferに入る。sourceはbufferに入った分なくなる
        match source.read_until(opt.line_ending, &mut buffer) {
            Ok(n) => {
                // コマンド実行後この条件が満たされて、stoppedがtrueになる
                // 結果をbufferに改行ごとに入れて、sourceの中身がなくなったらbreak
                if n == 0 {
                    break;
                }

                if buffer.ends_with(&[b'\r', b'\n']) {
                    buffer.pop();
                    buffer.pop();
                } else if buffer.ends_with(&[b'\n']) || buffer.ends_with(&[b'\0']) {
                    buffer.pop();
                }
                // thread::sleep_ms(3000);

                let item = Item::new(
                    String::from_utf8_lossy(&buffer),
                    opt.use_ansi_color,
                    &opt.transform_fields,
                    &opt.matching_fields,
                    &opt.delimiter,
                    (run_num, index),
                );

                {
                    // save item into pool
                    // ReaderControlのitemsフィールド, ArcでSpinLockなVec
                    // TUIのrowを管理している?
                    let mut vec = items.lock();
                    vec.push(Arc::new(item));
                    index += 1;
                }

                if stopped.load(Ordering::SeqCst) {
                    break;
                }
            }
            Err(_err) => {} // String not UTF8 or other error, skip.
        }
    }

    stopped.store(true, Ordering::Relaxed); // -cオプションのコマンド終了時に上述で立ち上げたthreadのwhile条件から抜けさせる

    // TODO ここの存在意義
    while !command_stopped.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(5));
    }
}
