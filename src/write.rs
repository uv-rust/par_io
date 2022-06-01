//! Parallel async file write.
use std::fs::File;
use std::ops::Fn;
use std::sync::mpsc::channel;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;
use core::fmt::Debug;

// -----------------------------------------------------------------------------
// TYPES

type Senders = Vec<Sender<Message>>;
type Buffer = Vec<u8>;
type Offset = u64;
#[derive(Clone)]
struct Config {
    offset: Offset,
    consumers: Senders,
    producer_tx: Sender<Message>,
}
// Using the same type to communicate between producers and consumers.
type ProducerConfig = Config;
type ConsumerConfig = Config;
type ProducerId = u64;
type NumProducers = u64;
enum Message {
    Consume(ConsumerConfig, Buffer), // sent to consumers
    Produce(ProducerConfig, Buffer), // sent to producers
    End(ProducerId, NumProducers),   // sent from producers to all consumers
                                     // to signal end of transmission
    Error(ProducerError)                    // sent from producer to consumers to signal
                                     // error
}

// Moving a generic Fn instance requires customization
type Producer<T, E> = dyn Fn(
    &mut Vec<u8>, // <- buffer to write to
    &T,           // <- client data
    u64,          // <- file offset (where data is written)
) -> Result<(), E>;
struct FnMove<T, E> {
    f: Arc<Producer<T, E>>,
}

/// Error generated by producers.
#[derive(Debug)]
pub struct ProducerError {
    pub msg: String,
    pub offset: u64
}

/// Error type containing errors generated by the producer and consumer threads and I/O operations.
#[derive(Debug)]
pub enum WriteError {
    /// Error generated by producer including producer callback.
    Producer(ProducerError),
    /// `std::io::Error` generated by consumer.
    IO(std::io::Error),
    /// Other errors
    Other(String),
}

/// Simple conversion from string to write error.
fn to_write_err(err: String) -> WriteError {
    WriteError::Other(err)
}

/// Fn is wrapped inside an FnMove struct so that it can be moved
impl<T, E> FnMove<T, E> {
    fn call(&self, buf: &mut Vec<u8>, t: &T, a: u64) -> Result<(), E> {
        (self.f)(buf, t, a)
    }
}

unsafe impl<T, E> Send for FnMove<T, E> {}

// -----------------------------------------------------------------------------
/// Select target consumer given current producer ID. Round-robin scheme.
fn select_tx(
    _i: usize,
    previous_consumer_id: usize,
    num_consumers: usize,
    _num_producers: usize,
) -> usize {
    (previous_consumer_id + 1) % num_consumers
}

/// -----------------------------------------------------------------------------
/// Separate file writing from data production using the producer-consumer model
/// and a fixed number of pre-allocated buffers to keep memory usage constant.
///
/// * thread *i* sends data by generated by callback function to thread *j*
/// * thread *j* writes data to file and sends consumed buffer back to thread *i* so that
///   it can be reused
///
/// The number of buffers used equals the number of producers times the number
/// of buffers per producer, regardless of the number of chunks generated.
///
/// ## Arguments
/// * `filename` - file to read
/// * `num_producers` - number of producers = number of producer threads
/// * `num_consumers` - number of consumers = number of consumer threads
/// * `chunks_per_producer` - number of chunks per producer = number of data generation tasks per producer
/// * `producer` - function generating data
/// * `client_data` - data to be passed to producer function
/// * `num_buffers_per_producer` - number of buffers per producer
///
/// Callback signature:
///
/// ```ignore
/// type Producer<T> = dyn Fn(&mut Vec<u8>, // <- buffer to write to
///                           &T,           // <- client data
///                           u64           // <- file offset (where data is written)
///                          ) -> Result<(), String>;
/// ```
// -----------------------------------------------------------------------------
// Write data to file.
// Internally data is subdivided as follows:
// ```ignore
// |||..........|.....||..........|.....||...>> ||...|.|||
//    <---1----><--2->                            <3><4>
//    <-------5------>                            <--6->
//    <-------------------------7---------------------->
// ```
// 1. task_chunk_size
// 2. last_task_chunk_size
// 3. last_producer_task_chunk_size
// 4. last_producer_last_task_chunk_size
// 5. producer_chunk_size
// 6. last_producer_chunk_size
// 7. total_size
pub fn write_to_file<T: 'static + Clone + Send, E: 'static + Send + Debug>(
    filename: &str,
    num_producers: u64,
    num_consumers: u64,
    chunks_per_producer: u64,
    producer: Arc<Producer<T, E>>,
    client_data: T,
    num_buffers_per_producer: u64,
    total_size: usize,
) -> Result<usize, WriteError> {
    let total_size = total_size as u64;
    let producer_chunk_size = (total_size + num_producers - 1) / num_producers;
    let last_producer_chunk_size = total_size - (num_producers - 1) * producer_chunk_size;
    let task_chunk_size = (producer_chunk_size + chunks_per_producer - 1) / chunks_per_producer;
    let last_task_chunk_size = producer_chunk_size - (chunks_per_producer - 1) * task_chunk_size;
    let last_prod_task_chunk_size =
        (last_producer_chunk_size + chunks_per_producer - 1) / chunks_per_producer;
    let last_last_prod_task_chunk_size =
        last_producer_chunk_size - (chunks_per_producer - 1) * last_prod_task_chunk_size;
    let file = File::create(filename).map_err(|err| to_write_err(err.to_string()))?;
    file.set_len(total_size).map_err(|err| to_write_err(err.to_string()))?;
    drop(file);
    let tx_producers = build_producers(
        num_producers,
        total_size,
        chunks_per_producer,
        producer,
        client_data,
    );
    let (tx_consumers, consumers_handles) = match build_consumers(num_consumers, filename) {
        Ok(r) => r,
        Err(err) => {
            return Err(err);
        }
    };
    let reserved_size = last_task_chunk_size
        .max(last_last_prod_task_chunk_size)
        .max(task_chunk_size);
    launch(
        tx_producers,
        tx_consumers,
        producer_chunk_size,
        last_producer_chunk_size,
        task_chunk_size,
        chunks_per_producer,
        reserved_size as usize,
        num_buffers_per_producer,
    )?;

    let mut bytes_consumed = 0;
    for h in consumers_handles {
        match h.join() {
            Ok(n) => match n {
                Ok(bytes) => {
                    bytes_consumed += bytes;
                }
                Err(err) => {
                    return Err(err);
                }
            },
            Err(err) => {
                return Err(WriteError::Other(format!("{:?}", err)));
            }
        }
    }
    Ok(bytes_consumed)
}

// -----------------------------------------------------------------------------
/// Build producers and return array of Sender objects.
fn build_producers<T: 'static + Clone + Send, E: 'static + Send + Debug>(
    num_producers: u64,
    total_size: u64,
    chunks_per_producer: u64,
    f: Arc<Producer<T, E>>,
    data: T,
) -> Senders {
    let mut tx_producers: Senders = Senders::new();
    let producer_chunk_size = (total_size + num_producers - 1) / num_producers;
    let last_producer_chunk_size = total_size - (num_producers - 1) * producer_chunk_size;
    let task_chunk_size = (producer_chunk_size + chunks_per_producer - 1) / chunks_per_producer;
    let last_prod_task_chunk_size =
        (last_producer_chunk_size + chunks_per_producer - 1) / chunks_per_producer;
    // currently producers exit after sending all data, and consumers might try
    // to send data back to disconnected producers, ignoring the returned
    // send() error;
    // another option is to have consumers return and 'End' signal when done
    // consuming data and producers exiting after al the consumers have
    // returned the signal
    for i in 0..num_producers {
        let (tx, rx) = channel();
        tx_producers.push(tx);
        let mut offset = producer_chunk_size * i;
        let end_offset = if i != num_producers - 1 {
            offset + producer_chunk_size
        } else {
            offset + last_producer_chunk_size
        };
        use Message::*;
        let cc = FnMove { f: f.clone() };
        let data = data.clone();
        thread::spawn(move || -> Result<(), String> {
            let mut prev_consumer = i as usize;
            while let Ok(Produce(mut cfg, mut buffer)) = rx.recv() {
                let chunk_size = if i != num_producers - 1 {
                    task_chunk_size.min(end_offset - offset)
                } else {
                    last_prod_task_chunk_size.min(end_offset - offset)
                };
                assert!(buffer.capacity() >= chunk_size as usize);
                unsafe {
                    buffer.set_len(chunk_size as usize);
                }
                let num_consumers = cfg.consumers.len();
                // to support multiple consumers per producer we need to keep track of
                // the destination, by adding the element into a Set and notify all
                // of them when the producer exits
                let c = select_tx(
                    i as usize,
                    prev_consumer,
                    num_consumers,
                    num_producers as usize,
                );
                prev_consumer = c;

                match cc.call(&mut buffer, &data, offset as u64) {
                    Err(err) => {
                        (0..cfg.consumers.len()).for_each(|c| {
                            let _ = cfg.consumers[c].send(Error(ProducerError{ msg: format!("{:?}", err), offset: offset}));
                        });
                        return Err(format!("{:?}", err)); 
                    }
                    Ok(()) => {
                        cfg.offset = offset;
                        offset += buffer.len() as u64;
                        if let Err(err) = cfg.consumers[c].send(Consume(cfg.clone(), buffer)) {
                            return Err(format!(
                                "Cannot send buffer to consumer - {}",
                                err.to_string()
                            ));
                        }
                        if offset >= end_offset {
                            // signal the end of stream to consumers
                            (0..cfg.consumers.len()).for_each(|x| {
                                // consumer might have exited already
                                let _ = cfg.consumers[x].send(End(i, num_producers));
                            });
                            break;
                        }
                    }
                }
            }
            return Ok(());
        });
    }
    tx_producers
}

// -----------------------------------------------------------------------------
/// Build consumers and return tuple of (Sender objects, JoinHandles)
fn build_consumers(
    num_consumers: u64,
    file_name: &str,
) -> Result<(Senders, Vec<JoinHandle<Result<usize, WriteError>>>), WriteError> {
    let mut consumers_handles = Vec::new();
    let mut tx_consumers = Vec::new();
    for _i in 0..num_consumers {
        let (tx, rx) = channel();
        tx_consumers.push(tx);
        use Message::*;
        let file_name = file_name.to_owned();
        let h = thread::spawn(move || {
            let file = File::options()
                .write(true)
                .open(&file_name)
                .map_err(|err| WriteError::IO(err))?;
            let mut producers_end_signal_count = 0;
            let mut bytes = 0;
            loop {
                // consumers tx endpoints live inside the ReadData instance
                // sent along messages, when producers finish sending data
                // all transmission endpoints die resulting in recv()
                // failing and consumers exiting
                if let Ok(msg) = rx.recv() {
                    match msg {
                        Error(err) => {
                            return Err(WriteError::Producer(err));
                        }
                        Consume(cfg, buffer) => {
                            bytes += buffer.len();
                            write_bytes_at(&buffer, &file, cfg.offset)?; 
                            if let Err(_err) = cfg.producer_tx.send(Produce(cfg.clone(), buffer)) {
                                // senders might have already exited at this point after having added
                                // data to the queue
                                // from Rust docs
                                //A send operation can only fail if the receiving end of a channel is disconnected, implying that the data could never be received
                                // TBD
                                //break;
                            }
                        }
                        End(_prod_id, num_producers) => {
                            producers_end_signal_count += 1;
                            if producers_end_signal_count >= num_producers {
                                break;
                            }
                        }
                        _ => {
                            panic!("Wrong message type");
                        }
                    }
                } else {
                    // we do not care if the communication channel was closed
                    // since it only happen when the producer is finished
                    // of an error elsewhere occurred
                    //break;
                }
            }
            return Ok(bytes);
        });
        consumers_handles.push(h);
    }
    Ok((tx_consumers, consumers_handles))
}

// -----------------------------------------------------------------------------
/// Launch computation by sending messages to transmission endpoints of producer
/// channels.
/// In order to keep memory usage constant, buffers are sent to consumers and
/// returned to the producer who sent them.
/// One producer can send messages to multiple consumers.
/// To allow for asynchronous data consumption, a consumers needs to be able
/// to consume the data in a buffer while the producer is writing data to a different
/// buffer and therefore more than one buffer per producer is required for
/// the operation to perform asynchronously.
fn launch(
    tx_producers: Senders,
    tx_consumers: Senders,
    producer_chunk_size: u64,
    task_chunk_size: u64,
    last_producer_task_chunk_size: u64,
    chunks_per_producer: u64,
    reserved_size: usize,
    num_buffers_per_producer: u64,
) -> Result<(), WriteError> {
    let num_buffers_per_producer = num_buffers_per_producer;
    let num_producers = tx_producers.len() as u64;
    for i in 0..num_producers {
        let tx = tx_producers[i as usize].clone();
        let offset = (i as u64) * producer_chunk_size;
        //number of messages/buffers to be sent to each producer's queue before
        //the computation starts
        let num_buffers = chunks_per_producer.min(num_buffers_per_producer);
        for _ in 0..num_buffers {
            let mut buffer: Vec<u8> = Vec::new();
            let chunk_size = if i != num_producers - 1 {
                task_chunk_size
            } else {
                last_producer_task_chunk_size
            };
            buffer.reserve(2 * reserved_size);
            unsafe {
                buffer.set_len(chunk_size as usize);
            }
            let cfg = ProducerConfig {
                offset: offset,
                producer_tx: tx.clone(),
                consumers: tx_consumers.clone(),
            };
            tx.send(Message::Produce(cfg, buffer))
                .map_err(|err| WriteError::Other(err.to_string()))?
        }
    }
    Ok(())
}
#[cfg(any(windows))]
fn write_bytes_at(buffer: &Vec<u8>, file: &File, offset: u64) -> Result<(), String> {
    use std::os::windows::fs::FileExt;
    file.seek_write(buffer, offset)
        .map_err(|err| err.to_string())?;
}

#[cfg(any(unix))]
fn write_bytes_at(buffer: &Vec<u8>, file: &File, offset: u64) -> Result<(), WriteError> {
    use std::os::unix::fs::FileExt;
    file.write_all_at(buffer, offset)
        .map_err(|err| WriteError::IO(err))?;
    Ok(())
}