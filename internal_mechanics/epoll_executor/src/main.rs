use {
    std::{
        collections::{HashMap,VecDeque},
        os::unix::io::RawFd,
        pin::Pin,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex,
        },
        task::{Context, RawWaker, RawWakerVTable, Waker},
        thread,
    },
    nix::{
        sys::{
            epoll::{epoll_create1, epoll_ctl, epoll_wait, EpollCreateFlags, EpollEvent, EpollFlags, EpollOp},
            eventfd::{eventfd, EfdFlags},
        },
        unistd::{read, write},
    },
};


// Pin: 自己参照を持つ型をmoveから守る
// Context: Wakerを渡すためのラッパー
trait SimpleFuture{
    type Output;
    fn poll(self:Pin<&mut Self>,cx: &mut Context)->Poll<Self::Output>; 
}

#[derive(Debug)]
enum Poll<T>{
    Ready(T),
    Pending,
}


type BoxFuture = Box<dyn SimpleFuture<Output = &'static str> + Send>;
fn boxed<F>(fut: F) -> Pin<Box<dyn SimpleFuture<Output = &'static str> + Send>>
where
    F: SimpleFuture<Output = &'static str> + Send + 'static,
{
    Pin::from(Box::new(fut) as Box<dyn SimpleFuture<Output = &'static str> + Send>)
}
// Task /////////////////////////////////////////////////////////////////////////////////////////
struct Task{
    name: String,
    futures: Mutex<Vec<Option<Pin<BoxFuture>>>>,
    executor: Arc<ExecutorInner>,
}

impl Task{
    //自分自身をExecutorのqueueにpushする
    fn schedule(self: &Arc<Self>){
        self.executor.queue.lock().unwrap().push_back(self.clone());

        // eventfdに書いてExecutorを起こす
        let data=1u64.to_ne_bytes();
        write(self.executor.eventfd_fd,&data).unwrap();
    }

    fn poll(self: Arc<Self>){
        let mut futs=self.futures.lock().unwrap();
        if !futs.is_empty(){
            let mut fut_slot=futs.remove(0);
            if let Some(mut fut)=fut_slot.take(){
                let waker=create_waker(self.clone());
                let mut ctx=Context::from_waker(&waker);

                let res=fut.as_mut().poll(&mut ctx);
                match res{
                    Poll::Ready(val)=>{
                        println!("{}",val);
                        if !futs.is_empty(){
                            self.schedule();
                        }else{
                            println!("Task {} completed!",self.name);
                        }
                    }
                    Poll::Pending=>{
                        futs.insert(0,Some(fut));
                    }
                }
            }
        }
    }
}

// Spawner ///////////////////////////////////////////////////////////////////////////
struct Spawner{
    inner: Arc<ExecutorInner>,
}

impl Spawner{
    fn new(executor: &Executor)->Self{
        Self{
            inner: executor.inner.clone(),
        }
    }
    fn spawn(&self,name:&str, futures:Vec<Option<Pin<BoxFuture>>>){
        let task=Arc::new(Task{
            name: name.to_string(),
            futures: Mutex::new(futures),
            executor: self.inner.clone(),
        });

        self.inner.queue.lock().unwrap().push_back(task);

        // Executorに通知
        let data=1u64.to_ne_bytes();
        write(self.inner.eventfd_fd,&data).unwrap();
    }

}

// Executor ///////////////////////////////////////////////////////////////////////////
//ExecutorInnerは実行待ちのタスクを管理する
//複数スレッドからタスクが追加、取り出しされないようにする
struct ExecutorInner{
    queue: Mutex<VecDeque<Arc<Task>>>, //同じタスクを共有
    eventfd_fd: RawFd,
}

//同じキューを共有
struct Executor{
    inner: Arc<ExecutorInner>,
}

impl Executor{
    fn new()->Self{
        let eventfd_fd=eventfd(0,EfdFlags::empty()).unwrap();
        Self {
            inner: Arc::new(ExecutorInner {
                queue: Mutex::new(VecDeque::new()),
                eventfd_fd,
             })
        }
    }

    fn run(&self) {
        loop {
            let mut buf = [0u8; 8];
            read(self.inner.eventfd_fd, &mut buf).unwrap();
        
            while let Some(task) = self.inner.queue.lock().unwrap().pop_front() {
                task.poll();
            }
        }
    }

}

// Waker ///////////////////////////////////////////////////////////////////////////////////
fn create_waker(task: Arc<Task>) -> Waker{
    // Wakerのクローンをつくる
    unsafe fn clone(data: *const ()) -> RawWaker {
        let arc = Arc::from_raw(data as *const Task); //from_rawしたarcはdropすると参照カウントが-1になる
        let arc_clone = arc.clone();
        std::mem::forget(arc); //ドロップした後に参照カウンタを-1しない
        RawWaker::new(data, &VTABLE)
    }

    // Taskを再スケジューリングしてWakerを消費する
    unsafe fn wake(data: *const()){
        let task=Arc::from_raw(data as *const Task);
        task.schedule();
    }

    // Wakerは消費されない
    unsafe fn wake_by_ref(data: *const()){
        let task=Arc::from_raw(data as *const Task);
        task.schedule();
        std::mem::forget(task);
    }

    //参照カウントを1減らす
    unsafe fn drop(data: *const()){
        //let _=Arc::from_raw(data as *const Task);
    }

    //clone,wake,wake_by_ref,dropがWakerに紐づく
    static VTABLE: RawWakerVTable=RawWakerVTable::new(clone,wake,wake_by_ref,drop);

    let ptr=Arc::into_raw(task) as *const(); //Arc<Task>を生ポインタ化
    let raw=RawWaker::new(ptr,&VTABLE); //RawWakerを作成
    unsafe {Waker::from_raw(raw)} //Waker::from_rawで安全なWakerに変換
}

// epoll //////////////////////////////////////////////////////////////////////////////
struct Epoll{
    epoll_fd:RawFd,
    callbacks: Arc<Mutex<HashMap<u64,Box<dyn Fn()+Send>>>>, //key=tolen(u64) value=callback関数
}

impl Epoll{
    fn new()->Epoll{
        let epoll_fd=epoll_create1(EpollCreateFlags::empty()).unwrap();
        Epoll{
            epoll_fd,
            callbacks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn add_fd<F>(&self,fd:RawFd,token:u64,callback:F)
    where F:Fn()+Send+'static,
    {
        let mut ev=EpollEvent::new(EpollFlags::EPOLLIN,token);
        epoll_ctl(self.epoll_fd,EpollOp::EpollCtlAdd,fd,&mut ev).unwrap();

        self.callbacks.lock().unwrap().insert(token,Box::new(callback));
    }

    fn start_loop(&self){
        let epoll_fd=self.epoll_fd;
        let callbacks=self.callbacks.clone();

        thread::spawn(move||{
            let mut events=vec![EpollEvent::empty();10];
            loop{
                let n=epoll_wait(epoll_fd,&mut events,-1).unwrap();
                for ev in &events[..n]{
                    let token=ev.data();
                    if let Some(cb)=callbacks.lock().unwrap().get(&token){
                        cb();
                    }
                }
            }
        });
    }
}



// Future ////////////////////////////////////////////////////////////////////////////////
#[derive(Clone)]
struct StdinFuture{
    is_ready: Arc<AtomicBool>,
    waker: Arc<Mutex<Option<Waker>>>,
}

impl StdinFuture{
    fn new()->Self{
        Self{
            is_ready:Arc::new(AtomicBool::new(false)),
            waker:Arc::new(Mutex::new(None)),
        }
    }

    fn set_ready(&self){
        self.is_ready.store(true,Ordering::SeqCst);
        if let Some(w)=self.waker.lock().unwrap().take(){
            w.wake();
        }
    }
}

impl SimpleFuture for StdinFuture{
    type Output=&'static str;

    fn poll(mut self:Pin<&mut Self>,cx: &mut Context)->Poll<Self::Output>{
        if self.is_ready.load(Ordering::SeqCst){ //もしis_readyがtrueならReadyを返す
            let mut buf=String::new();
            std::io::stdin().read_line(&mut buf).unwrap();
            let n:i32=buf.trim().parse().unwrap_or(0);
            println!("stdin future result: {}",n+10);
            Poll::Ready("stdin future done")
        }else{
            *self.waker.lock().unwrap()=Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

// main ///////////////////////////////////////////////////////////////////////////////////////////////////
fn main(){
    let executor=Executor::new();
    let spawner=Spawner::new(&executor);

    let stdin_future=Arc::new(StdinFuture::new());
    let stdin_future_clone=stdin_future.clone();

    let epoll=Epoll::new();

    epoll.add_fd(0,0,move||{ //epollが検知したらset_readyを呼び出す
        stdin_future_clone.set_ready();
    });

    let evfd=executor.inner.eventfd_fd;
    epoll.add_fd(evfd,evfd as u64,||{

    });

    epoll.start_loop();

    let stdin_future_boxed 
    = Some(boxed((*stdin_future).clone()));
    spawner.spawn("stdin task", vec![stdin_future_boxed]);

    executor.run();
}