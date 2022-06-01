# `par_io` Parallel I/O library

Simple library, with no dependencies, to read and write files in parallel
implementing the producer-consumer model.

Synchronous calls to `pread` and `pwrite` inside reader and writer threads are used
to transfer data.

No async runtime is used since the actual file I/O is synchronous and task
distribution and execution is controlled by the library through direct calls
to Rust's *thread* and *mpsc* APIs.

`Fn` type objects are passed by client code to the library and invoked by
producer or consumer threads to generate or consume data.

Memory buffers are created once and reused across producers and consumers, so
no memory allocation happens after buffer creation, unless performed in the 
callback objects provided by the client code.

Total memory consumption is equal to:
 
*Memory allocated in client code* +

*(buffer size) x (number of buffers per producer) x (number of producers)*

When only one buffer per producer is provided consumers must wait for producer
to send the buffer and producers must wait for consumers to send the buffer
back and therefore per-thread producer-consumer execution is synchronous.

When multiple buffers per producer are used consumers can generate data while
consumers are processing it reading from a separate buffers, and therefore full
asynchronous execution is possible.

Current implementation allows to set the number of chunks per producer and the
number of buffers per producer with

*(number of buffers) <= (number of chunks)*

In the future it will be possible to explicitly specify the maximum amount of
memory used.

### Reading

(producer = reader)

    1. the file is subdivided into chunks
    2. each chunk is read by a separate producer thread
    3. the producer thread extracts a buffer from a queue and fills it with the data from the file
    4. the filled buffer is sent to a consumer thread (round-robin scheduling)
    5. the consumer thread passes a reference to the buffer to a consumer callback received from client code
    6. the return value from the callback is stored into an array
    7. the buffer is moved back to the thread that sent it
    8. all the return values from all the consumer threads are merged into a single array and returned to client code

### Writing

(consumer = writer)

    1. producer threads extract buffer from queue
    2. mutable reference to buffer is passed to producer callback received from client code
    3. result of callback invocation is checked: 
       1. no error: buffer is sent to consumer threads (round robin scheduling);
       2. error: error is sent to consumer threads which then terminate immediately
    4. consumer threads receive the buffer and the file offset and store the data into file
    5. buffer is moved back to the producer thread that sent it
    6. each consumer thread returns the number of bytes written to file  
    7. the results from all consumer threads are merged into a single array returned to client code


## Usage

`read_file` and `write_to_file` functions are used for read and write operations.

The `read_file` function returns a vector of 

  *size = (number of chunks per producer) x (number of producers)*

with each element containing the return value of the callback object consuming
the data. It is up to the callback objects to return errors as needed.

The `write_to_file` function returns a `Result` instance containing the number of
bytes written or an `Err(String)` instance.
In case the producer's callback fails with an error, such error is forwarded to
consumers which immediately exit returning the received error.

## Parallel reading example

```rust
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
    // callback function
    let consume = |buffer: &[u8],   // buffer containing data from file
                   data: &String,   // custom data
                   chunk_id: u64,   // chunk id 
                   num_chunks: u64, // number of chunks per producer
                   _offset: u64     // file offset
     -> Result<usize, String> {
        std::thread::sleep(std::time::Duration::from_secs(1));
        println!(
            "Consumer: {}/{} {} {}",
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
        std::sync::Arc::new(consume), // <- callback
        tag,                          // <- client data passed to callback
        num_buffers_per_producer,
    ) {
        Ok(v) => {
            let bytes_consumed = v
                .iter()
                .fold(0, |acc, x| if let (_, Ok(b)) = x { acc + b } else { acc });
            assert_eq!(bytes_consumed, len as usize);
        }
        Err(err) => {
            eprintln!("{}", err.to_string());
        }
    }
```

## Parallel writing example

```rust
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
    let producer = |buffer: &mut Vec<u8>, _tag: &String, _offset: u64| -> Result<(), String> {
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
            use par_io::write::{WriteError, ProducerError, ConsumerError};
            match err {
                WriteError::Producer(ProducerError{msg, offset}) => {
                    eprintln!("Producer error: {} at {}", msg, offset);
                },
                WriteError::Consumer(ConsumerError{msg}) => {
                    eprintln!("Consumer error: {}", msg);
                },
                WriteError::Other(err) => {
                    eprintln!("Error: {}", err);
                },
            }
        }
    }
}
```