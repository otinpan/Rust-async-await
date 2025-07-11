# C言語の非同期処理
## select 
```c
#include <stdio.h>
#include<sys/select.h>
#include<unistd.h>

int main(){
    fd_set rfds;
    int max_fd=0;

    FD_ZERO(&rfds);
    FD_SET(0,&rfds);

    max_fd=0;

    int ret=select(max_fd+1,&rfds,NULL,NULL,NULL); // データが到着するかを確認する
    if(ret<0){
        perror("select");
        return 1;
    }

    if (FD_ISSET(0,&rfds)){ // selectの結果を確認
        char buf[100];
        read(0,buf,sizeof(buf));
        printf("入力がありました: %s\n",buf);
    }
    return 0;
}
```
* `FD_ZERO`、`FD_SET`、`select()`を呼ぶ
* カーネルに
    - fd=0を監視してほしい
    - データが来たら教えて
と依頼する
* 標準入力にデータがまだないなら`select()`を呼んだスレッドはwait queueに入れられてスリープする。その際CPUは使わない
* データが到着したら
    - カーネルが`select()`をwakeする
    - `select()`が戻る
* `ret`にデータが到着したFDの数が記録される
* `FD_ISSET()`でどのFDかを確認する  

`select`が戻ってくるまで次の行に進めないで、止まっている。すなわち同期的になる。

```c
#include <stdio.h>
#include <unistd.h>
#include <sys/select.h>
#include <string.h>

int main() {
    int pipefd[2];
    pipe(pipefd); // pipefd[0] = 読み取り, pipefd[1] = 書き込み

    // とりあえずパイプにデータを書き込む
    write(pipefd[1], "pipe data\n", 10);

    fd_set rfds;
    int max_fd;

    while (1) {
        FD_ZERO(&rfds);

        // 標準入力を監視
        FD_SET(0, &rfds);

        // パイプの読み取り側も監視
        FD_SET(pipefd[0], &rfds);

        // 最大のFDを決定
        max_fd = pipefd[0] > 0 ? pipefd[0] : 0;

        int ret = select(max_fd + 1, &rfds, NULL, NULL, NULL);
        if (ret < 0) {
            perror("select");
            return 1;
        }

        // 標準入力にデータが来た？
        if (FD_ISSET(0, &rfds)) {
            char buf[100];
            int n = read(0, buf, sizeof(buf) - 1);
            if (n > 0) {
                buf[n] = '\0';
                printf("標準入力からの入力: %s", buf);
            }
        }

        // パイプにデータが来た？
        if (FD_ISSET(pipefd[0], &rfds)) {
            char buf[100];
            int n = read(pipefd[0], buf, sizeof(buf) - 1);
            if (n > 0) {
                buf[n] = '\0';
                printf("パイプからの入力: %s", buf);
            }
        }
    }

    return 0;
}
```
