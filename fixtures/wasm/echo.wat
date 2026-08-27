(module
  (memory (export "memory") 1)

  (global $heap (mut i32) (i32.const 1024))

  (data (i32.const 2048) "{\"status\":200,\"body\":\"ok\"}")

  (func (export "alloc") (param $size i32) (result i32)
    (local $ptr i32)

    global.get $heap
    local.tee $ptr

    local.get $size
    i32.add

    global.set $heap

    local.get $ptr
  )

  (func (export "handle") (param $ptr i32) (param $len i32) (result i64)
    i64.const 8796093022234
  )
)