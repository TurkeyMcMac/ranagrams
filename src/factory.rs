use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::sync::mpsc::Receiver;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(PartialEq, Eq, Debug)]
enum BossMessage {
    Go,
    Stop,
}

#[derive(PartialEq, Eq, Debug)]
enum WorkerMessage {
    WakeUp,
    Slain,
    Sleeping(usize),
}

pub trait WorkerFun<I: Send + 'static>: Send + Sync + 'static {
    fn improve(&self, I) -> Vec<I>;
    fn inspect(&self, &I) -> bool;
}

pub fn manufacture<I, W>(
    roster: usize,
    materials: Vec<I>,
    fun: Arc<W>,
) -> (Receiver<Option<I>>, Arc<AtomicBool>)
where
    I: Send + 'static,
    W: WorkerFun<I>,
{
    let conveyor_belt = Arc::new(Mutex::new(materials));
    let (container, truck) = mpsc::channel::<Option<I>>();
    let workers = Arc::new(Mutex::new(Vec::with_capacity(roster)));
    let (manager, stamps) = mpsc::channel::<WorkerMessage>();
    let kill_switch = Arc::new(AtomicBool::new(false));
    (0..roster)
        .map(|i| {
            let belt = conveyor_belt.clone();
            let container = container.clone();
            let manager = manager.clone();
            let fun = fun.clone();
            let (worker, in_box) = mpsc::channel::<BossMessage>();
            let kill_switch = kill_switch.clone();
            workers.lock().unwrap().push(worker);
            thread::spawn(move || {
                for message in in_box {
                    if message == BossMessage::Stop {
                        break;
                    }
                    if kill_switch.load(Ordering::Relaxed) {
                        manager.send(WorkerMessage::Slain).ok();
                        break;
                    }
                    while let Some(stuff) = {
                        let mut temp = belt.lock().unwrap();
                        temp.pop()
                    } {
                        if kill_switch.load(Ordering::Relaxed) {
                            manager.send(WorkerMessage::Slain).ok();
                            break;
                        }
                        let widgets = fun.improve(stuff);
                        if !widgets.is_empty() {
                            let mut belt = belt.lock().unwrap();
                            for widget in widgets {
                                if fun.inspect(&widget) {
                                    // this bit of work is done, send it to its final destination
                                    container.send(Some(widget)).ok(); // ship it
                                } else {
                                    belt.push(widget); // put this back on the conveyor belt
                                }
                            }
                            manager.send(WorkerMessage::WakeUp).ok();
                        }
                    }
                    manager.send(WorkerMessage::Sleeping(i)).ok(); // send I'm empty message
                }
            })
        })
        .collect::<Vec<_>>();
    thread::spawn(move || {
        let mut idled: Vec<usize> = Vec::with_capacity(roster);
        for w in workers.lock().unwrap().iter() {
            w.send(BossMessage::Go).ok();
        }
        for message in stamps {
            match message {
                WorkerMessage::Slain => {
                    container.send(None).ok();
                    let foo = workers.lock().unwrap();
                    for &i in idled.iter() {
                        if let Some(w) = foo.get(i) {
                            w.send(BossMessage::Go).ok();
                        }
                    }
                    break;
                }
                WorkerMessage::WakeUp => {
                    let foo = workers.lock().unwrap();
                    for &i in idled.iter() {
                        if let Some(w) = foo.get(i) {
                            w.send(BossMessage::Go).ok();
                        }
                    }
                    idled.clear();
                }
                WorkerMessage::Sleeping(i) => {
                    idled.push(i);
                    if idled.len() == roster {
                        container.send(None).ok();
                        for worker in workers.lock().unwrap().iter() {
                            worker.send(BossMessage::Stop).ok();
                        }
                    }
                }
            }
        }
    });
    (truck, kill_switch)
}