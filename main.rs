use std::io::{self, BufRead};
use std::collections::HashMap;
use std::time::SystemTime;
use std::fs;

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

    fn write_to(&self, w: &mut impl io::Write) -> io::Result<()> {
        for x in self.tokens.iter() {
            w.write(&x.to_le_bytes())?;
        }
        Ok(())
    }

    fn read_from(r: &mut impl io::Read) -> io::Result<Freq> {
        let mut result = Freq::new();
        for token in result.tokens.iter_mut() {
            let mut freq_buf = [0; 2];
            r.read(&mut freq_buf)?;
            *token = u16::from_le_bytes(freq_buf);
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

    fn read_from(r: &mut impl io::Read) -> io::Result<Self> {
        let mut result = Self::new();
        let mut context_buf = [0; 8];
        while r.read(&mut context_buf)? == 8  {
            let context = u64::from_le_bytes(context_buf);
            let freq = Freq::read_from(r)?;
            result.model.insert(context, freq);
        }
        Ok(result)
    }

    fn write_to(&self, w: &mut impl io::Write) -> io::Result<()> {
        for (context, freq) in self.model.iter() {
            w.write(&context.to_le_bytes())?;
            freq.write_to(w)?;
        }
        Ok(())
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

fn main() {
    let mut lcg = LCG::new(SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs());

    // println!("Reading pre-trained model");
    // let model = Model::read_from(&mut io::BufReader::new(fs::File::open("model.bin").unwrap())).unwrap();

    println!("Training the model...");
    let mut model = Model::new();
    {
        let file_path = "twitch.log";
        println!("  {file_path}");
        for line in io::BufReader::new(fs::File::open(file_path).unwrap()).lines() {
            for (context, next) in Slicer::new(line.unwrap().into_bytes()) {
                model.push(context, next)
            }
        }
    }
    {
        let file_path = "discord.log";
        println!("  {file_path}");
        for line in io::BufReader::new(fs::File::open(file_path).unwrap()).lines() {
            for (context, next) in Slicer::new(line.unwrap().into_bytes()) {
                model.push(context, next)
            }
        }
    }

    // println!("Saving the model");
    // model.write_to(&mut io::BufWriter::new(fs::File::create("model.bin").unwrap())).unwrap()

    println!("Generating text...");
    for _ in 0..100 {
        let mut context = 0;
        let mut buffer = Vec::new();
        const LIMIT: usize = 1024;
        while let Some(x) = model.random(context, &mut lcg) {
            if buffer.len() >= LIMIT {
                break
            }
            buffer.push(x);
            context_push(&mut context, x);
        }
        println!("{}", std::str::from_utf8(&buffer).unwrap());
    }
}
