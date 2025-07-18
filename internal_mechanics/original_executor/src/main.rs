use {
    std::{
        thread,
        pin::Pin,
        sync::{Arc, Mutex},
        thread::{sleep},
        time::Duration,
        collections::{VecDeque},
        task::{RawWaker,RawWakerVTable,Waker,Context},
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
                            println!("Task {} copleted!",self.name);
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
    }
}

// Executor ///////////////////////////////////////////////////////////////////////////
//ExecutorInnerは実行待ちのタスクを管理する
//複数スレッドからタスクが追加、取り出しされないようにする
struct ExecutorInner{
    queue: Mutex<VecDeque<Arc<Task>>>, //同じタスクを共有
}

//同じキューを共有
struct Executor{
    inner: Arc<ExecutorInner>,
}

impl Executor{
    fn new()->Self{
        Self {
            inner: Arc::new(ExecutorInner {
                queue: Mutex::new(VecDeque::new()),
             })
        }
    }

    fn run(&self){
        loop{
            let task_opt=self.inner.queue.lock().unwrap().pop_front();

            if let Some(task)=task_opt{
                task.poll();
            }else{
                thread::sleep(Duration::from_millis(10));
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


// Future ////////////////////////////////////////////////////////////////////////////////
struct MyFuture{
    state: MyState,
}


#[derive(Debug)]
enum MyState{
    Start,
    Middle,
    End,
}

impl MyFuture{
    fn new()->Self{
        Self{
            state:MyState::Start,
        }
    }
}

impl SimpleFuture for MyFuture{
    type Output=&'static str;
    fn poll(mut self:Pin<&mut Self>,cx:&mut Context)->Poll<Self::Output>{
        let this=self.as_mut().get_mut();
        match this.state{
            MyState::Start=>{
                println!("Start");
                println!("Yielded: Start -> Middle");
                this.state=MyState::Middle;
                cx.waker().wake_by_ref(); //自分自身をqueueにpushする
                Poll::Pending
            }
            MyState::Middle=>{
                println!("Middle");
                println!("Yielded: Middle -> End");
                this.state=MyState::End;
                cx.waker().wake_by_ref(); // dorpしない
                Poll::Pending
            }
            MyState::End=>{
                println!("End");
                Poll::Ready("finished")
            }

        }
    }
}

struct SleepFuture{
    is_sleep: Arc<Mutex<bool>>,
    sleep_time: Duration,
    spawned: bool,
    waker: Arc<Mutex<Option<Waker>>>,
}

impl SleepFuture{
    fn new(duration:Duration)->Self{
        let sleeper=Self{
            is_sleep: Arc::new(Mutex::new(true)),
            sleep_time: duration,
            spawned: false,
            waker: Arc::new(Mutex::new(None)),
        };
        sleeper
    }
}

impl SimpleFuture for SleepFuture{
    type Output=&'static str;
    fn poll(mut self:Pin<&mut Self>,cx:&mut Context)->Poll<Self::Output>{
        let this=self.as_mut().get_mut();
        if *this.is_sleep.lock().unwrap(){
            println!("...zzz");
            if !this.spawned{
                this.spawned=true;
                let is_sleep_clone=this.is_sleep.clone();
                let waker_clone=this.waker.clone();
                let sleep_time=this.sleep_time;

                thread::spawn(move||{
                    thread::sleep(sleep_time);
                    {
                        *is_sleep_clone.lock().unwrap()=false;
                    }
                    // スレッドがsleepを終えたタイミングにwakerがあればw.wake()が呼ばれる
                    if let Some(w)=waker_clone.lock().unwrap().take(){
                        w.wake(); //スレッドではwake()を呼ぶ
                    }
                });
            }
            *this.waker.lock().unwrap()=Some(cx.waker().clone());
            Poll::Pending
        }else{
            Poll::Ready("wake from sleep!")
        }
    }
}


struct AsyncBlockFuture{
    state: AsyncBlockState,
    sleep_future: Option<Pin<BoxFuture>>,
}

#[derive(Debug)]
enum AsyncBlockState{
    Start,
    Sleeping,
    End,
}

impl AsyncBlockFuture{
    fn new(duration: Duration)->Self{
        Self{
            state: AsyncBlockState::Start,
            sleep_future: Some(boxed(SleepFuture::new(duration))),
        }
    }
}

impl SimpleFuture for AsyncBlockFuture{
    type Output=&'static str;
    fn poll(mut self:Pin<&mut Self>,cx:&mut Context)->Poll<Self::Output>{
        let this=self.as_mut().get_mut();
        match this.state{
            AsyncBlockState::Start=>{
                println!("start");
                this.state=AsyncBlockState::Sleeping;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            AsyncBlockState::Sleeping=>{
                let fut=this.sleep_future.as_mut().unwrap();
                match fut.as_mut().poll(cx){
                    Poll::Pending=>{
                        Poll::Pending
                    }
                    Poll::Ready(val)=>{
                        println!("{}",val);
                        this.state=AsyncBlockState::End;
                        cx.waker().wake_by_ref();
                        Poll::Pending
                    }
                }
            }
            AsyncBlockState::End=>{
                Poll::Ready("end")
            }
        }
    }
}

// main ///////////////////////////////////////////////////////////////////////////////////////////////////
fn main(){
    let executor=Executor::new();
    let spawner=Spawner::new(&executor);
    let my_future=MyFuture::new();


    let sleep_future_3secs=SleepFuture::new(Duration::from_secs(3));
    let async_block_future=AsyncBlockFuture::new(Duration::from_secs(6));

    let sleep_future_3secs=Some(boxed(sleep_future_3secs));
    let async_block_future=Some(boxed(async_block_future));

    spawner.spawn("3秒sleepするタスク",vec![sleep_future_3secs]);
    spawner.spawn("Start、Sleep、Endの状態を持つタスク",vec![async_block_future]);

    executor.run();
}


