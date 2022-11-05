use std::collections::HashMap;
use std::time::SystemTime;
use std::fs;
use std::env;
use std::process::exit;

// Stolen from https://en.wikipedia.org/wiki/Linear_congruential_generator
// Using the values of MMIX by Donald Knuth
struct LCG {
    state: u64
}

impl LCG {
    fn new(seed: u64) -> Self {
        Self {state: seed}
    }

    fn random_u32(&mut self) -> u32 {
        const RAND_A: u64 = 6364136223846793005;
        const RAND_C: u64 = 1442695040888963407;
        (self.state, _) = self.state.overflowing_mul(RAND_A);
        (self.state, _) = self.state.overflowing_add(RAND_C);
        return (self.state>>32) as u32;
    }
}

#[derive(Debug)]
struct Freq {
    tokens: [u16; 256],
}

impl Freq {
    fn new() -> Self {
        Self { tokens: [0; 256] }
    }

    fn push(&mut self, x: u8) {
        if self.tokens[x as usize] < u16::MAX {
            self.tokens[x as usize] += 1;
        } else {
            for i in 0..self.tokens.len() {
                if i != x as usize && self.tokens[i] > 0 {
                    self.tokens[i] -= 1;
                }
            }
        }
    }

    fn random(&self, lcg: &mut LCG) -> Option<u8> {
        let mut sum: usize = 0;
        for t in self.tokens.iter() {
            sum += *t as usize;
        }

        if sum > 0 {
            let index = (lcg.random_u32() as usize)%sum;
            let mut psum: usize = 0;
            for i in 0..self.tokens.len() {
                psum += self.tokens[i] as usize;
                if psum > index {
                    return Some(i as u8)
                }
            }
        }
        None
    }
}

#[derive(Debug)]
struct Model {
    model: HashMap<u64, Freq>,
}

impl Model {
    fn new() -> Self {
        Self {
            model: HashMap::new()
        }
    }

    fn random(&self, context: u64, lcg: &mut LCG) -> Option<u8> {
        self.model.get(&context).and_then(|freq| freq.random(lcg))
    }

    fn push(&mut self, context: u64, next: u8) {
        match self.model.get_mut(&context) {
            Some(freq) => freq.push(next),
            None => {
                let mut freq = Freq::new();
                freq.push(next);
                self.model.insert(context, freq);
            }
        }
    }
}

struct Slicer {
    bytes: Vec<u8>,
    window: u64,
    cursor: usize,
}

impl Slicer {
    fn new(bytes: Vec<u8>) -> Self {
        Self{bytes, window: 0, cursor: 0}
    }
}

impl Iterator for Slicer {
    type Item = (u64, u8);

    fn next(&mut self) -> Option<Self::Item> {
        if self.cursor >= self.bytes.len() {
            return None
        }

        let result = self.window;
        let next = self.bytes[self.cursor];
        self.window = (self.window<<8)|(next as u64);
        self.cursor += 1;

        return Some((result, next));
    }
}

fn context_push(context: &mut u64, x: u8) {
    *context = ((*context)<<8)|(x as u64);
}

fn usage(program: &str) {
    eprintln!("Usage: {program} <subcommand> [arguments]");
    eprintln!("Subcommands:");
    eprintln!("    gen <input.txt> [limit]    generate random text based on a model trained from <input.txt>");
    eprintln!("    stats <input.txt>          print some stats of the model that is trained from <input.txt>");
}

fn main() {
    let mut lcg = LCG::new(
        SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(
            |d| d.as_secs()
        ).unwrap_or_else(
            |e| e.duration().as_secs()
        )
    );

    let mut args = env::args();
    let program = args.next().expect("Program name should be always present");

    let subcommand = args.next().unwrap_or_else(|| {
        usage(&program);
        eprintln!("ERROR: no subcommand is provided");
        exit(1);
    });

    match subcommand.as_str() {
        "gen" => {
            let file_path = args.next().unwrap_or_else(|| {
                usage(&program);
                eprintln!("ERROR: no input file is provided");
                exit(1);
            });

            let limit = args.next().map(|text| {
                text.parse::<usize>().unwrap_or_else(|_| {
                    eprintln!("ERROR: limit must be an integer. Sadly `{text}` does not look like an integer.");
                    exit(1)
                })
            }).unwrap_or(1024);

            println!("Training the model...");
            let mut model = Model::new();
            let bytes = fs::read(&file_path).unwrap_or_else(|err| {
                eprintln!("ERROR: could not read file {file_path}: {err}");
                exit(1)
            });
            for (context, next) in Slicer::new(bytes) {
                model.push(context, next)
            }


            println!("Generating text...");
            println!("------------------------------");
            let mut context = 0;
            let mut buffer = Vec::new();
            while let Some(x) = model.random(context, &mut lcg) {
                if buffer.len() >= limit {
                    break
                }
                buffer.push(x);
                context_push(&mut context, x);
            }
            println!("{}", std::str::from_utf8(&buffer).unwrap());
        },
        "stats" => todo!(),
        _ => {
            usage(&program);
            eprintln!("ERROR: unknown subcommand `{subcommand}`");
            exit(1);
        }
    }
}
