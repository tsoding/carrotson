use std::collections::HashMap;
use std::time::SystemTime;
use std::fs;
use std::io;
use std::env;
use std::process::exit;

struct LCG {
    state: u64
}

impl LCG {
    fn new(seed: u64) -> Self {
        Self {state: seed}
    }

    fn random_u32(&mut self) -> u32 {
        // Stolen from https://en.wikipedia.org/wiki/Linear_congruential_generator
        // Using the values of MMIX by Donald Knuth
        const RAND_A: u64 = 6364136223846793005;
        const RAND_C: u64 = 1442695040888963407;
        (self.state, _) = self.state.overflowing_mul(RAND_A);
        (self.state, _) = self.state.overflowing_add(RAND_C);
        return (self.state>>32) as u32;
    }
}

#[derive(Debug)]
struct Freq {
    tokens: Vec<(u8, u32)>,
}

fn read_u8(r: &mut impl io::Read) -> io::Result<u8> {
    let mut buf = [0; 1];
    r.read_exact(&mut buf)?;
    Ok(u8::from_le_bytes(buf))
}

fn read_u32(r: &mut impl io::Read) -> io::Result<u32> {
    let mut buf = [0; 4];
    r.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

fn read_u64(r: &mut impl io::Read) -> io::Result<u64> {
    let mut buf = [0; 8];
    r.read_exact(&mut buf)?;
    Ok(u64::from_le_bytes(buf))
}

impl Freq {
    fn branching(&self) -> usize {
        return self.tokens.len();
    }

    fn new() -> Self {
        Self { tokens: Vec::new() }
    }

    fn push(&mut self, x: u8) {
        let mut found = false;
        for (y, p) in self.tokens.iter_mut() {
            if *y == x {
                *p += 1;
                found = true;
                break;
            }
        }

        if !found {
            self.tokens.push((x, 1))
        }
    }

    fn random(&self, lcg: &mut LCG) -> Option<u8> {
        let sum: usize = self.tokens.iter().map(|(_, p)| *p as usize).sum();

        if sum > 0 {
            let index = (lcg.random_u32() as usize)%sum;
            let mut psum: usize = 0;
            for (y, p) in self.tokens.iter() {
                psum += *p as usize;
                if psum > index {
                    return Some(*y)
                }
            }
        }
        None
    }

    fn write_to(&self, w: &mut impl io::Write) -> io::Result<()> {
        w.write_all(&(self.tokens.len() as u8).to_le_bytes())?;
        for (x, p) in self.tokens.iter() {
            w.write_all(&x.to_le_bytes())?;
            w.write_all(&p.to_le_bytes())?;
        }
        Ok(())
    }

    fn read_from(r: &mut impl io::Read) -> io::Result<Self> {
        let mut result = Self::new();
        let count = read_u8(r)?;
        for _ in 0..count {
            let x = read_u8(r)?;
            let p = read_u32(r)?;
            result.tokens.push((x, p));
        }
        Ok(result)
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

    fn write_to(&self, w: &mut impl io::Write) -> io::Result<()> {
        w.write_all(&(self.model.len() as u64).to_le_bytes())?;
        for (context, freq) in self.model.iter() {
            w.write_all(&context.to_le_bytes())?;
            freq.write_to(w)?;
        }
        w.flush()?;
        Ok(())
    }

    fn read_from(r: &mut impl io::Read) -> io::Result<Self> {
        let mut result = Self::new();
        let count = read_u64(r)?;
        result.model.reserve(count as usize);
        for _ in 0..count {
            let context = read_u64(r)?;
            let freq = Freq::read_from(r)?;
            result.model.insert(context, freq);
        }
        Ok(result)
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
    eprintln!("Usage: {program} <SUBCOMMANDS> [OPTIONS]");
    eprintln!("Subcommands:");
    eprintln!("    train <INPUT> <OUTPUT>     generate binary model file <OUTPUT> based on <INPUT>");
    eprintln!("    gen <FILE> [-l <LIMIT>]    generate random text based on a model trained from <FILE>");
    eprintln!("    stats <FILE>               print some stats of the model that is trained from <FILE>");
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

            println!("Loading the model from {file_path}...");
            let file = fs::File::open(&file_path).unwrap_or_else(|err| {
                eprintln!("ERROR: could not read from file {file_path}: {err}");
                exit(1);
            });
            let model = Model::read_from(&mut io::BufReader::with_capacity(200*1024*1024, file)).unwrap_or_else(|err| {
                eprintln!("ERROR: could not read from file {file_path}: {err}");
                exit(1);
            });

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
        "stats" => {
            let file_path = args.next().unwrap_or_else(|| {
                usage(&program);
                eprintln!("ERROR: no input file is provided");
                exit(1);
            });

            println!("Training the model...");
            let mut model = Model::new();
            let bytes = fs::read(&file_path).unwrap_or_else(|err| {
                eprintln!("ERROR: could not read file {file_path}: {err}");
                exit(1)
            });
            for (context, next) in Slicer::new(bytes) {
                model.push(context, next)
            }

            let mut max_branching = usize::MIN;
            let mut avg_branching = 0f32;
            for (_context, freq) in model.model.iter() {
                let branching = freq.branching();
                max_branching = std::cmp::max(max_branching, branching);
                avg_branching += branching as f32;
            }
            avg_branching /= model.model.len() as f32;

            println!("Records count: {}", model.model.len());
            println!("Maximum branching: {max_branching}");
            println!("Average branching: {avg_branching}");
        }
        "train" => {
            let input_file_path = args.next().unwrap_or_else(|| {
                usage(&program);
                eprintln!("ERROR: no input file is provided");
                exit(1);
            });
            let output_file_path = args.next().unwrap_or_else(|| {
                usage(&program);
                eprintln!("ERROR: no output file is provided");
                exit(1);
            });

            println!("Training the model...");
            let mut model = Model::new();
            let bytes = fs::read(&input_file_path).unwrap_or_else(|err| {
                eprintln!("ERROR: could not read file {input_file_path}: {err}");
                exit(1)
            });
            for (context, next) in Slicer::new(bytes) {
                model.push(context, next)
            }

            println!("Saving the model to {output_file_path}...");
            let output_file = fs::File::create(&output_file_path).unwrap_or_else(|err| {
                eprintln!("ERROR: could not write file {output_file_path}: {err}");
                exit(1)
            });
            model.write_to(&mut io::BufWriter::new(output_file)).unwrap_or_else(|err| {
                eprintln!("ERROR: could not write file {output_file_path}: {err}");
                exit(1)
            });
        }
        _ => {
            usage(&program);
            eprintln!("ERROR: unknown subcommand `{subcommand}`");
            exit(1);
        }
    }
}
