// Model-free process fixture; never linked into the product.
use std::io::{self, Read, Write};
fn main() -> io::Result<()> {
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let mut header = [0; 14];
    input.read_exact(&mut header)?;
    let length = u32::from_le_bytes(header[10..14].try_into().unwrap()) as usize;
    let mut path = vec![0; length];
    input.read_exact(&mut path)?;
    output.write_all(&[0x80])?;
    output.flush()?;
    let mut samples = 0_u32;
    let mut fail_finish = false;
    let mut created = 0_u64;
    let mut finished = 0_u64;
    let mut stall_shutdown = false;
    loop {
        let mut tag = [0];
        if input.read_exact(&mut tag).is_err() || tag[0] == 3 {
            if stall_shutdown { std::thread::sleep(std::time::Duration::from_secs(10)); }
            return Ok(());
        }
        let mut id = [0; 8];
        input.read_exact(&mut id)?;
        if tag[0] == 1 {
            let mut count = [0; 4];
            input.read_exact(&mut count)?;
            let count = u32::from_le_bytes(count);
            let mut pcm = vec![0; count as usize * 4];
            input.read_exact(&mut pcm)?;
            let mode = f32::from_le_bytes(pcm[..4].try_into().unwrap());
            stall_shutdown |= mode == -0.25;
            pcm.fill(0);
            if mode == 1.0 { std::thread::sleep(std::time::Duration::from_secs(10)); }
            if mode == 0.5 { return Ok(()); }
            if mode == 0.75 {
                output.write_all(&[0x81])?;
                output.write_all(&u64::from_le_bytes(id).saturating_sub(1).to_le_bytes())?;
                output.flush()?;
                continue;
            }
            if mode == 0.25 {
                output.write_all(&[0x82])?;
                output.write_all(&id)?;
                output.write_all(&[0, 0, 0, 0, 0, 255, 255, 255, 127])?;
                output.flush()?;
                continue;
            }
            fail_finish = mode == -0.5;
            if mode == -1.0 {
                output.write_all(&[0x83])?;
                output.write_all(&id)?;
                output.write_all(&[3])?;
            } else {
                if samples == 0 { created += 1; }
                samples += count;
                output.write_all(&[0x81])?;
                output.write_all(&id)?;
            }
        } else if tag[0] == 4 {
            output.write_all(&[0x85])?;
            output.write_all(&id)?;
            for value in [created, finished, finished, 0, u64::from(samples > 0), u64::from(created > 0)] {
                output.write_all(&value.to_le_bytes())?;
            }
        } else {
            finished += 1;
            output.write_all(&[0x82])?;
            output.write_all(&id)?;
            output.write_all(&[u8::from(!fail_finish)])?;
            output.write_all(&(samples / 16).to_le_bytes())?;
            output.write_all(&0_u32.to_le_bytes())?;
            samples = 0;
        }
        output.flush()?;
    }
}
