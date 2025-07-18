use std::time;
use tokio::time as tokio_time;

#[tokio::main]
async fn main() {
    // 5秒スリープするタスク
    let five_secs_sleeper = tokio::spawn(async {
        println!("start 5secs sleep");
        let five_secs = time::Duration::from_secs(5);
        tokio_time::sleep(five_secs).await;
        println!("wake from 5secs sleep!");
    });

    // 2秒スリープするタスク
    let two_secs_sleeper = tokio::spawn(async {
        println!("start 2secs sleep");
        let two_secs = time::Duration::from_secs(2);
        tokio_time::sleep(two_secs).await;
        println!("wake from 2secs sleep!");
    });

    // Hello を表示するタスク
    let print_hello = tokio::spawn(async {
        println!("Hello");
    });

    let _ = tokio::join!(five_secs_sleeper, two_secs_sleeper, hello_sleeper);
}
