//! # Parallel file I/O
//! This crate provides a simple interface to read and write from/to files in parallel.
//! Data consumption is decoupled from data production by having separate groups
//! of producer and consumer threads.
//! 
//! Files are read and written through synchronous `pread` and `pwrite` function calls.
//!
//! The same buffers are reused for both production and consumption and consumed
//! buffers are sent back to the producer, thus the total amount of memory
//! used is equal to the number of producers times the number of buffers per producer.
//!
//! When reading, the client callback function acts as the consumer and when writing
//! it acts as the producer.
//!
//! ## Reading
//! Producer threads read data chunks from the file and send them to the consumer threads
//! which pass the data to a client callback function.
//!
//! Client code can use the `read_file` function to read the file specifying a callback
//! function that will be called for each chunk read from the file.
//!
//! The signature of the callback object passed to `read_file` is:
//!
//! ```ignore
//! type Consumer<T, R> =
//!     dyn Fn(&[u8], // data read from file
//!              &T,  // client data    
//!              u64, // chunk id   
//!              u64, // number of chunks
//!              u64  // file offset (where data is read from)
//!           ) -> R;
//! ```
//!
//! ### Example
//!
//! ```ignore
//!  ...
//!  let consume = |buffer: &[u8], // <- client callback
//!                   data: &Data, // <- generic parameter
//!                   chunk_id: u64,
//!                   num_chunks: u64,
//!                   _offset: u64|
//!     -> Result<usize, String> {
//!        std::thread::sleep(std::time::Duration::from_secs(1));
//!        println!(
//!            "Consumer: {}/{} {} {}",
//!            chunk_id,
//!            num_chunks,
//!            data.msg,
//!            buffer.len()
//!        );
//!        Ok(buffer.len())
//!    };
//!    let tag = "TAG".to_string();
//!    match read_file(
//!        &filename,
//!        num_producers,
//!        num_consumers,
//!        chunks_per_producer,
//!        std::sync::Arc::new(consume),
//!        tag,
//!        num_buffers_per_producer,
//!    ) {
//!        Ok(v) => {
//!            let bytes_consumed = v
//!                .iter()
//!                .fold(0, |acc, x| if let (_, Ok(b)) = x { acc + b } else { acc });
//!            assert_eq!(bytes_consumed, len as usize);
//!        }
//!        Err(err) => {
//!            use par_io::read::ReadError;
//!            match err {
//!                ReadError::IO(err) => {
//!                    eprintln!("IO error: {:?}", err);
//!                },
//!                ReadError::Send(err) => {
//!                    eprintln!("Send error: {:?}", err);
//!                },
//!                ReadError::Other(err) => {
//!                    eprintln!("Error: {:?}", err);
//!                }
//!            }
//!        }
//!    }
//! ```
//!
//! ## Writing
//! Producer threads invoke a client callback to generate data which is then sent to consumer threads
//! which write data to file.
//!
//! The signature of the callback object passed to the `write_to_file` function is:
//! ```ignore
//! type Producer<T> = dyn Fn(&mut Vec<u8>, // <- buffer to write to
//!                           &T,           // <- client data
//!                           u64           // <- file offset (where data is written)
//!                          ) -> Result<(), String>;
//! ```
//!
//!  ### Example
//! ```ignore
//! ...
//!    
//!    let producer = |buffer: &mut Vec<u8>, // <- client callback
//!                    _tag: &String, // <- generic parameter
//!                     _offset: u64| -> Result<(), String> {
//!        std::thread::sleep(std::time::Duration::from_secs(1));
//!        *buffer = vec![1_u8; buffer.len()];
//!        Ok(())
//!    };
//!    let data = "TAG".to_string();
//!    match write_to_file(
//!        &filename,
//!        num_producers,
//!        num_consumers,
//!        chunks_per_producer,
//!        std::sync::Arc::new(producer),
//!        data,
//!        num_buffers_per_producer,
//!        buffer_size,
//!    ) {
//!        Ok(bytes_consumed) => {
//!            let len = std::fs::metadata(&filename)
//!                .expect("Cannot access file")
//!                .len();
//!            assert_eq!(bytes_consumed, len as usize);
//!            std::fs::remove_file(&filename).expect("Cannot delete file");
//!        },
//!        Err(err) => {
//!            use par_io::write::{WriteError, ProducerError, ConsumerError};
//!            match err {
//!                WriteError::Producer(ProducerError{msg, offset}) => {
//!                    eprintln!("Producer error: {} at {}", msg, offset);
//!                },
//!                WriteError::IO(err) => {
//!                    eprintln!("I/O error: {:?}", err);
//!                },
//!                WriteError::Other(err) => {
//!                    eprintln!("Error: {:?}", err);
//!                },
//!            }
//!        }
//!    }
pub mod read;
pub mod write;
