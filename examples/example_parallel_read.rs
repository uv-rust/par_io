//! A simple example of parallel reading from file. 
//! Data is read by producer threads and sent to consumer threads which pass the 
//! data to a client callback for consumption.
//! 
//! Input:
//! 
//! * input file name
//! * number of producer threads
//! * number of consumer threads 
//! * number of chunks (== tasks) per producer
//! * number of buffers per producer
//!
//! Usage:
//! ```ignore
//! cargo run --example example_parallel_read <input file name> 12 4 3 2
//! ```
use par_io::read::read_file;
pub fn main() {
    let filename = std::env::args().nth(1).expect("Missing file name");
    let len = std::fs::metadata(&filename)
        .expect("Error reading file size")
        .len();
    let num_producers: u64 = std::env::args()
        .nth(2)
        .expect("Missing num producers")
        .parse()
        .unwrap();
    let num_consumers: u64 = std::env::args()
        .nth(3)
        .expect("Missing num consumers")
        .parse()
        .unwrap();
    let chunks_per_producer: u64 = std::env::args()
        .nth(4)
        .expect("Missing num chunks per producer")
        .parse()
        .unwrap();
    let num_buffers_per_producer: u64 = if let Some(p) = std::env::args().nth(5) {
        p.parse().expect("Wrong num tasks format")
    } else {
        2
    };
    let consume = |buffer: &[u8],
                   data: &String,
                   chunk_id: u64,
                   num_chunks: u64,
                   _offset: u64|
     -> Result<usize, String> {
        std::thread::sleep(std::time::Duration::from_secs(1));
        println!(
            "Consumer: {}/{} {} buffer length: {}",
            chunk_id,
            num_chunks,
            data,
            buffer.len()
        );
        Ok(buffer.len())
    };
    let tag = "TAG".to_string();
    match read_file(
        &filename,
        num_producers,
        num_consumers,
        chunks_per_producer,
        std::sync::Arc::new(consume),
        tag,
        num_buffers_per_producer,
    ) {
        Ok(v) => {
            let bytes_consumed = v
                .iter()
                .fold(0, |acc, x| if let (_, Ok(b)) = x { acc + b } else { acc });
            assert_eq!(bytes_consumed, len as usize);
        }
        Err(err) => {
            use par_io::read::ReadError;
            match err {
                ReadError::IO(err) => {
                    eprintln!("IO error: {:?}", err);
                },
                ReadError::Send(err) => {
                    eprintln!("Send error: {:?}", err);
                },
                ReadError::Other(err) => {
                    eprintln!("Error: {:?}", err);
                }
            }
        }
    }
}
