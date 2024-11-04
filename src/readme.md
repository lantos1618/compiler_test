# compiler_tests

This repo is for testing the Cranelift compiler functionality.

## Features to Test

### Basic Features
- function definition ✓
```c
int main() {
    return 0;
}
```

- if/else statements ✓
```c
int main() {
    if (0) {
        return 1;
    } else {
        return 0;
    }
}
```

- function calls ✓
```c 
int add(int a, int b) {
    return a + b;   
}
int main() {
    return add(1, 2);
}
```

- structs ✓
```c
struct Point {
    int x;
    int y;
};

int main() {
    Point p;
    p.x = 1;
    p.y = 2;
    return p.x + p.y;
}
```

- while loops ✓
```c
int main() {
    int i = 0;
    while (i < 10) {
        i++;
    }
    return i;
}
```

### Additional Features to Test

- floating point operations
```c
int main() {
    float a = 3.14;
    double b = 2.718;
    
    // Basic arithmetic
    float sum = a + b;
    float diff = a - b;
    float prod = a * b;
    float div = a / b;
    
    // Comparisons
    if (a > b) {
        return 1;
    }
    
    // Type conversions
    int x = (int)a;
    float y = (float)x;
    
    return x;
}
```

- floating point functions
```c
double sqrt_approx(double x) {
    // Newton's method
    double guess = x / 2.0;
    for (int i = 0; i < 5; i++) {
        guess = (guess + x/guess) / 2.0;
    }
    return guess;
}

int main() {
    double result = sqrt_approx(16.0);
    return (int)result;  // Should return ~4
}
```

- switch statements
```c
int main() {
    int x = 1;
    switch (x) {
        case 0: return 0;
        case 1: return 1;
        default: return -1;
    }
}
```

- arrays and array operations
```c
int main() {
    int arr[5];
    arr[0] = 1;
    arr[1] = 2;
    return arr[0] + arr[1];
}
```

- pointer operations
```c
int main() {
    int x = 42;
    int* ptr = &x;
    *ptr = 24;
    return x;
}
```

- global variables
```c
int global = 42;

int main() {
    global += 1;
    return global;
}
```

- string operations
```c
int main() {
    char str[] = "hello";
    return str[0];  // should return 'h'
}
```

- compound assignments
```c
int main() {
    int x = 5;
    x += 3;
    x *= 2;
    return x;
}
```

- logical operators
```c
int main() {
    int a = 1;
    int b = 0;
    return a && b || !b;
}
```

- bitwise operations
```c
int main() {
    int x = 5;  // 101
    int y = 3;  // 011
    return (x & y) | (x ^ y);
}
```

### Advanced Features

- function pointers
```c
int add(int a, int b) { return a + b; }
int sub(int a, int b) { return a - b; }

int main() {
    int (*op)(int, int) = add;
    return op(5, 3);
}
```

- variadic functions
```c
#include <stdarg.h>

int sum(int count, ...) {
    va_list args;
    va_start(args, count);
    
    int total = 0;
    for (int i = 0; i < count; i++) {
        total += va_arg(args, int);
    }
    
    va_end(args);
    return total;
}
```

- typedef and enums
```c
typedef unsigned int uint;
enum Color { RED, GREEN, BLUE };

int main() {
    uint x = 42;
    enum Color c = RED;
    return x + c;
}
```
