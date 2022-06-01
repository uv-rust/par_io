//! A simple example of parallel writing to file. 
//! Data is generated in producer callback function and sent to writer threads
//! 
//! Input:
//! 
//! * memory buffer size
//! * output file name
//! * number of producer threads
//! * number of consumer threads, 
//! * number of chunks (== tasks) per producer
//! * number of buffers per producer
//! 
//! Usage:
//! ```ignore
//! cargo run --example example_parallel_write 2041 tmp-out 12 4 3 2
//! ```
use par_io::write::write_to_file;
pub fn main() {
    let buffer_size: usize = std::env::args()
        .nth(1)
        .expect("Missing buffer size")
        .parse()
        .expect("Wrong buffer size format");
    let filename = std::env::args().nth(2).expect("Missing file name");
    let num_producers: u64 = std::env::args()
        .nth(3)
        .expect("Missing num producers")
        .parse()
        .unwrap();
    let num_consumers: u64 = std::env::args()
        .nth(4)
        .expect("Missing num consumers")
        .parse()
        .unwrap();
    let chunks_per_producer: u64 = std::env::args()
        .nth(5)
        .expect("Missing num chunks per producer")
        .parse()
        .unwrap();
    let num_buffers_per_producer: u64 = if let Some(p) = std::env::args().nth(6) {
        p.parse().expect("Wrong num tasks format")
    } else {
        2
    };
    // Avoid overwriting existing file: exit if output file exists.
    if let Ok(_) = std::fs::metadata(&filename) {
        eprintln!("Error: file '{}' exists", filename);
        std::process::exit(1);
    }
    let producer = |buffer: &mut Vec<u8>, _tag: &String, offset: u64| -> Result<(), String> {
        std::thread::sleep(std::time::Duration::from_secs(1));
        println!("{:?}> Writing to offset {}", buffer.as_ptr(), offset);
        let len = buffer.len();
        // Warning: data need to be modified in place if not `buffer` will be re-allocated and not reused
        buffer.copy_from_slice(vec![1_u8; len].as_slice());
        Ok(())
    };
    let data = "TAG".to_string();
    match write_to_file(
        &filename,
        num_producers,
        num_consumers,
        chunks_per_producer,
        std::sync::Arc::new(producer),
        data,
        num_buffers_per_producer,
        buffer_size,
    ) {
        Ok(bytes_consumed) => {
            let len = std::fs::metadata(&filename)
                .expect("Cannot access file")
                .len();
            assert_eq!(bytes_consumed, len as usize);
            std::fs::remove_file(&filename).expect("Cannot delete file");
        },
        Err(err) => {
            use par_io::write::{WriteError, ProducerError};
            match err {
                WriteError::Producer(ProducerError{msg, offset}) => {
                    eprintln!("Producer error: {} at {}", msg, offset);
                },
                WriteError::IO(err) => {
                    eprintln!("I/O error: {:?}", err);
                },
                WriteError::Other(err) => {
                    eprintln!("Error: {}", err);
                },
            }
        }
    }
}
