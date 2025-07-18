use std::time;

#[tokio::main]
async fn main(){
    tokio::join!(async move{
        let ten_secs=time::Duration::from_secs(10);
        tokio::time::sleep(ten_secs).await;
        println!("wake!");
    });
}