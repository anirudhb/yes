use std::{
    fs::File,
    io::{stdin, stdout, Write},
};

use cranelift::{
    codegen::ir::FuncRef,
    frontend::{FunctionBuilder, FunctionBuilderContext},
    prelude::*,
};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module};

pub struct JIT {
    builder_context: FunctionBuilderContext,
    ctx: codegen::Context,
    //data_ctx: DataContext,
    module: JITModule,
}

impl JIT {
    pub fn new() -> Self {
        let mut flag_builder = settings::builder();
        // On at least AArch64, "colocated" calls use shorter-range relocations,
        // which might not reach all definitions; we can't handle that here, so
        // we require long-range relocation types.
        flag_builder.set("use_colocated_libcalls", "false").unwrap();
        flag_builder.set("is_pic", "true").unwrap();
        // flag_builder.set("opt_level", "speed_and_size").unwrap();
        flag_builder.set("opt_level", "none").unwrap();
        flag_builder.set("enable_verifier", "false").unwrap();
        let isa_builder = cranelift_native::builder().unwrap_or_else(|msg| {
            panic!("host machine is not supported: {}", msg);
        });
        let isa = isa_builder.finish(settings::Flags::new(flag_builder));
        let builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
        // let builder = JITBuilder::new(cranelift_module::default_libcall_names());
        let module = JITModule::new(builder);
        Self {
            builder_context: FunctionBuilderContext::new(),
            ctx: module.make_context(),
            //data_ctx: DataContext::new(),
            module,
        }
    }

    pub fn translate(&mut self, insns: &[OptimizedBFInstruction]) {
        // i64
        let int = self.module.target_config().pointer_type();

        // take in data ptr
        self.ctx.func.signature.params.push(AbiParam::new(int));
        self.ctx.func.signature.returns.push(AbiParam::new(int));

        let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.builder_context);

        // declare putchar
        let mut putchar_sig = self.module.make_signature();
        putchar_sig.params.push(AbiParam::new(int));
        // putchar_sig.returns.push(AbiParam::new(int));
        let putchar_id = self
            .module
            .declare_function("putchar", Linkage::Import, &putchar_sig)
            .unwrap();
        let putchar = self
            .module
            .declare_func_in_func(putchar_id, &mut builder.func);
        // let putchar_sigr = builder.import_signature(putchar_sig);
        // let putchar_extfd = ExtFuncData {
        //     name: ExternalName::user(0, 0),
        //     signature: putchar_sigr,
        //     colocated: false,
        // };
        // let putchar = builder.import_function(putchar_extfd);

        // // declare scanchar
        // let mut scanchar_sig = self.module.make_signature();
        // // scanchar_sig.params.push(AbiParam::new(int));
        // scanchar_sig.returns.push(AbiParam::new(int));
        // let scanchar_id = self
        //     .module
        //     .declare_function("scanchar", Linkage::Import, &scanchar_sig)
        //     .unwrap();
        // let scanchar = self
        //     .module
        //     .declare_func_in_func(scanchar_id, &mut builder.func);
        let mut getchar_sig = self.module.make_signature();
        getchar_sig.returns.push(AbiParam::new(int));
        let getchar_id = self
            .module
            .declare_function("getchar", Linkage::Import, &getchar_sig)
            .unwrap();
        let getchar = self
            .module
            .declare_func_in_func(getchar_id, &mut builder.func);
        // let scanchar_sigr = builder.import_signature(scanchar_sig);
        // let scanchar_extfd = ExtFuncData {
        //     name: ExternalName::user(0, 1),
        //     signature: scanchar_sigr,
        //     colocated: false,
        // };
        // let scanchar = builder.import_function(scanchar_extfd);

        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        // let mut data_ptr =;
        let data_ptr = Variable::new(0);
        builder.declare_var(data_ptr, int);
        builder.def_var(data_ptr, builder.block_params(entry_block)[0]);

        let mut trans = Translator {
            // plusone: builder.ins().iconst(int, 1),
            // minusone: builder.ins().iconst(int, -1),
            // /** i64's are EIGHT BYTES */
            // rightone: builder.ins().iconst(int, 8),
            // leftone: builder.ins().iconst(int, -8),
            builder,
            int,
            putchar,
            // scanchar,
            getchar,
            data_ptr,
            //module: &mut self.module,
        };

        for insn in insns {
            trans.translate_insn(insn);
            // let v = trans.translate_insn(insn);
            // trans.builder.def_var(data_ptr, v);
        }

        // emit the return
        let v2 = trans.builder.use_var(data_ptr);
        let l = trans.builder.ins().load(int, MemFlags::new(), v2, 0);

        trans.builder.ins().return_(&[l]);
        trans.builder.finalize();
    }

    pub fn jit(&mut self, insns: &[OptimizedBFInstruction]) -> *const u8 {
        self.translate(insns);
        println!("Translation done");

        let id = self
            .module
            .declare_function("bf", Linkage::Export, &self.ctx.func.signature)
            .unwrap();
        self.module
            .define_function(
                id,
                &mut self.ctx,
                &mut codegen::binemit::NullTrapSink {},
                &mut codegen::binemit::NullStackMapSink {},
            )
            .unwrap_or_else(|e| {
                println!("{:?}", e);
                println!("{}", self.ctx.func.display(self.module.isa()));
                // let mut s = String::new();
                // codegen::write_function(
                //     &mut s,
                //     &self.ctx.func,
                //     &DisplayFunctionAnnotations::default(),
                // )
                // .unwrap();
                panic!()
            });
        // println!("Debug function value:");
        // println!("{}", self.ctx.func.display(self.module.isa()));
        {
            let mut f = File::create("dis").unwrap();
            write!(f, "{}", self.ctx.func.display(self.module.isa())).unwrap();
        }
        println!("Written IR to dis");
        self.module.clear_context(&mut self.ctx);
        self.module.finalize_definitions();
        let code = self.module.get_finalized_function(id);
        code
    }
}

struct Translator<'a> {
    int: Type,
    builder: FunctionBuilder<'a>,
    //module: &'a mut JITModule,
    putchar: FuncRef,
    // scanchar: FuncRef,
    getchar: FuncRef,
    data_ptr: Variable,
    // minusone: Value,
    // plusone: Value,
    // rightone: Value,
    // leftone: Value,
}

impl<'a> Translator<'a> {
    fn translate_insn(&mut self, insn: &OptimizedBFInstruction) {
        use OptimizedBFInstruction::*;
        match insn {
            // DataPtrDecrement => {
            //     let dptr = self.builder.use_var(self.data_ptr);
            //     // let c = self.builder.ins().iconst(self.int, -1i64);
            //     // let r = self.builder.ins().iadd(dptr, c);
            //     // let r = self.builder.ins().iadd_imm(dptr, -1);
            //     let r = self.builder.ins().iadd(dptr, self.leftone);
            //     self.builder.def_var(self.data_ptr, r);
            // }
            // DataPtrIncrement => {
            //     let dptr = self.builder.use_var(self.data_ptr);
            //     // let c = self.builder.ins().iconst(self.int, 1i64);
            //     // let r = self.builder.ins().iadd(dptr, c);
            //     // let r = self.builder.ins().iadd_imm(dptr, 1);
            //     let r = self.builder.ins().iadd(dptr, self.rightone);
            //     self.builder.def_var(self.data_ptr, r);
            // }
            // DataValueDecrement => {
            //     let dptr = self.builder.use_var(self.data_ptr);
            //     // let c = self.builder.ins().iconst(self.int, -1i64);
            //     let l = self.builder.ins().load(self.int, MemFlags::new(), dptr, 0);
            //     // let a = self.builder.ins().iadd(c, l);
            //     // let a = self.builder.ins().iadd_imm(l, -1);
            //     let a = self.builder.ins().iadd(l, self.minusone);
            //     self.builder.ins().store(MemFlags::new(), a, dptr, 0);
            //     // dptr
            // }
            // DataValueIncrement => {
            //     let dptr = self.builder.use_var(self.data_ptr);
            //     // let c = self.builder.ins().iconst(self.int, 1i64);
            //     let l = self.builder.ins().load(self.int, MemFlags::new(), dptr, 0);
            //     // let a = self.builder.ins().iadd(c, l);
            //     // let a = self.builder.ins().iadd_imm(l, 1);
            //     let a = self.builder.ins().iadd(l, self.plusone);
            //     self.builder.ins().store(MemFlags::new(), a, dptr, 0);
            //     // dptr
            // }
            DataPtrModify(x) => {
                let dptr = self.builder.use_var(self.data_ptr);
                let a = self.builder.ins().iadd_imm(dptr, 8 * x);
                self.builder.def_var(self.data_ptr, a);
            }
            DataValueModify(x) => {
                let dptr = self.builder.use_var(self.data_ptr);
                let l = self.builder.ins().load(self.int, MemFlags::new(), dptr, 0);
                let a = self.builder.ins().iadd_imm(l, *x);
                self.builder.ins().store(MemFlags::new(), a, dptr, 0);
            }
            DataValuePutchar => {
                let dptr = self.builder.use_var(self.data_ptr);
                let l = self.builder.ins().load(self.int, MemFlags::new(), dptr, 0);
                self.builder.ins().call(self.putchar, &[l]);
                // dptr
            }
            DataValueScanchar => {
                let dptr = self.builder.use_var(self.data_ptr);
                // let c = self.builder.ins().iconst(self.int, 0);
                let s = self.builder.ins().call(self.getchar, &[]);
                let r = self.builder.inst_results(s)[0];
                self.builder.ins().store(MemFlags::new(), r, dptr, 0);
                // dptr
            }
            WhileDataValueNonZero(insns) => {
                let header_block = self.builder.create_block();
                let body_block = self.builder.create_block();
                let exit_block = self.builder.create_block();
                // let mut dptr = dptr;

                self.builder.ins().jump(header_block, &[]);
                self.builder.switch_to_block(header_block);

                let dptr2 = self.builder.use_var(self.data_ptr);
                let l = self.builder.ins().load(self.int, MemFlags::new(), dptr2, 0);
                self.builder.ins().brz(l, exit_block, &[]);
                self.builder.ins().jump(body_block, &[]);

                self.builder.switch_to_block(body_block);
                self.builder.seal_block(body_block);

                for insn in insns {
                    self.translate_insn(insn);
                    // let v = self.translate_insn(insn);
                    // self.builder.def_var(self.data_ptr, v);
                }
                self.builder.ins().jump(header_block, &[]);
                // let r = self.builder.use_var(self.data_ptr);

                self.builder.switch_to_block(exit_block);
                // self.builder.append_block_param(exit_block, self.int);
                // self.builder.append_block_param(exit_block, self.int);
                // let r = self.builder.block_params(exit_block);
                // let (r1, r2) = (r[0], r[1]);
                // self.builder.def_var(self.data_ptr, r1);
                // self.builder.ins().store(MemFlags::new(), r2, r1, 0);
                self.builder.seal_block(header_block);
                // let r = self.builder.use_var(self.data_ptr);
                // self.builder.def_var(self.data_ptr, r);
                self.builder.seal_block(exit_block);

                // dptr
            } // _ => todo!(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum BFInstruction {
    DataPtrIncrement,
    DataPtrDecrement,
    DataValueIncrement,
    DataValueDecrement,
    DataValuePutchar,
    DataValueScanchar,
    WhileDataValueNonZero(Vec<BFInstruction>),
}

#[derive(Debug)]
pub enum OptimizedBFInstruction {
    DataPtrModify(i64),
    DataValueModify(i64),
    DataValuePutchar,
    DataValueScanchar,
    WhileDataValueNonZero(Vec<OptimizedBFInstruction>),
}

impl OptimizedBFInstruction {
    pub fn optimize(
        mut insns: Vec<BFInstruction>,
        i: &mut u64,
        l: u64,
        top: bool,
    ) -> Vec<OptimizedBFInstruction> {
        let mut insns2 = Vec::new();
        let mut counter = 0;
        let mut ty = 0; // 1 = data ptr, 2 = value
        while insns.len() > 0 {
            let insn = insns.remove(0);
            *i += 1;
            if *i % 1000 == 0 {
                print!("\rInfo: optimized instruction {}/{}", i, l);
                stdout().flush().unwrap();
            }
            match insn {
                BFInstruction::DataPtrDecrement => {
                    if ty == 0 {
                        ty = 1;
                        counter = 0;
                    } else if ty != 1 {
                        insns2.push(OptimizedBFInstruction::DataValueModify(counter));
                        counter = 0;
                        ty = 1;
                    }
                    counter -= 1;
                }
                BFInstruction::DataPtrIncrement => {
                    if ty == 0 {
                        ty = 1;
                        counter = 0;
                    } else if ty != 1 {
                        insns2.push(OptimizedBFInstruction::DataValueModify(counter));
                        counter = 0;
                        ty = 1;
                    }
                    counter += 1;
                }
                BFInstruction::DataValueDecrement => {
                    if ty == 0 {
                        ty = 2;
                        counter = 0;
                    } else if ty != 2 {
                        insns2.push(OptimizedBFInstruction::DataPtrModify(counter));
                        counter = 0;
                        ty = 2;
                    }
                    counter -= 1;
                }
                BFInstruction::DataValueIncrement => {
                    if ty == 0 {
                        ty = 2;
                        counter = 0;
                    } else if ty != 2 {
                        insns2.push(OptimizedBFInstruction::DataPtrModify(counter));
                        counter = 0;
                        ty = 2;
                    }
                    counter += 1;
                }
                x => {
                    match ty {
                        1 => insns2.push(OptimizedBFInstruction::DataPtrModify(counter)),
                        2 => insns2.push(OptimizedBFInstruction::DataValueModify(counter)),
                        _ => {}
                    }
                    ty = 0;
                    insns2.push(match x {
                        BFInstruction::DataValuePutchar => OptimizedBFInstruction::DataValuePutchar,
                        BFInstruction::DataValueScanchar => {
                            OptimizedBFInstruction::DataValueScanchar
                        }
                        BFInstruction::WhileDataValueNonZero(inner) => {
                            OptimizedBFInstruction::WhileDataValueNonZero(Self::optimize(
                                inner, i, l, false,
                            ))
                        }
                        _ => unreachable!(),
                    })
                }
            }
        }
        match ty {
            1 => insns2.push(OptimizedBFInstruction::DataPtrModify(counter)),
            2 => insns2.push(OptimizedBFInstruction::DataValueModify(counter)),
            _ => {}
        }
        if top {
            print!("\rInfo: optimized instruction {}/{}", i, l);
            stdout().flush().unwrap();
        }
        insns2
    }

    pub fn walk_len(v: &[Self]) -> u64 {
        let mut len = 0;
        for x in v {
            match x {
                Self::WhileDataValueNonZero(ref v) => len += 1 + Self::walk_len(v.as_slice()),
                _ => len += 1,
            }
        }
        len
    }
}

impl BFInstruction {
    pub fn parse_char(c: char) -> Option<Self> {
        match c {
            '>' => Some(BFInstruction::DataPtrIncrement),
            '<' => Some(BFInstruction::DataPtrDecrement),
            '+' => Some(BFInstruction::DataValueIncrement),
            '-' => Some(BFInstruction::DataValueDecrement),
            '.' => Some(BFInstruction::DataValuePutchar),
            ',' => Some(BFInstruction::DataValueScanchar),
            _ => None,
        }
    }

    pub fn walk_len(v: &[Self]) -> u64 {
        let mut len = 0;
        for x in v {
            match x {
                Self::WhileDataValueNonZero(ref v) => len += 1 + Self::walk_len(v.as_slice()),
                _ => len += 1,
            }
        }
        len
    }
}

// pub struct ParserNestingState(Vec<BFInstruction>, Option<Box<ParserNestingState>>);

#[derive(Debug)]
pub struct Parser {
    // nesting: Option<ParserNestingState>,
    insns: Vec<BFInstruction>,
    s: Vec<char>,
    ptr: usize,
}

impl Parser {
    pub fn from_str(s: &str) -> Self {
        Self {
            // nesting: None,
            insns: Vec::new(),
            s: s.chars().collect(),
            ptr: 0,
        }
    }

    pub fn parse(&mut self) -> Vec<BFInstruction> {
        while self.ptr < self.s.len() {
            self.parse_step();
        }
        std::mem::take(&mut self.insns)
    }

    fn parse_step(&mut self) {
        match self.s[self.ptr] {
            c @ ('>' | '<' | '+' | '-' | '.' | ',') => {
                self.ptr += 1;
                self.insns.push(BFInstruction::parse_char(c).unwrap());
            }
            '[' => {
                self.ptr += 1;
                let mut saved_insns = std::mem::take(&mut self.insns);
                while self.s[self.ptr] != ']' && self.ptr < self.s.len() {
                    self.parse_step();
                    // insns.push(self.s[self.ptr]);
                    // self.ptr += 1;
                }
                let err = self.ptr >= self.s.len();
                self.ptr += 1; // account for ']'
                if err {
                    panic!("Unmatched open bracket, parser state = {:?}", self);
                }
                // self.ptr += 1;
                saved_insns.push(BFInstruction::WhileDataValueNonZero(std::mem::take(
                    &mut self.insns,
                )));
                self.insns = saved_insns;
            }
            // ']' omitted since parsed above^
            ']' => panic!("Unmatched close bracket, parser state = {:?}", self),
            _ => self.ptr += 1,
        }
    }
}

// // JIT bindings
// #[link(name = "c")]
// extern "C" {
//     pub fn putchar(c: u64);
//     pub fn getchar() -> u64;
// }

// #[no_mangle]
// pub extern "C" fn putchar(c: u64) {
//     print!("{}", unsafe { char::from_u32_unchecked(c as _) });
//     stdout().flush().unwrap();
// }

// #[no_mangle]
// pub extern "C" fn scanchar() -> u64 {
//     let mut arr = [0; 1];
//     stdin().read_exact(&mut arr).unwrap();
//     arr[0] as _
// }

type BFJitFunction = extern "C" fn(*mut u64) -> u64;

// fn yes() {
//     let c = scanchar();
//     println!("char = {}", c);
// }

fn main() {
    // Disable buffering

    // 'A' = 65
    // let code = "+++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++.";
    // let code = "++++++++[>++++[>++>+++>+++>+<<<<-]>+>+>->>+[<]<-]>>.>---.+++++++..+++.>>.<-.<.+++.------.--------.>>+.>++.";
    // let code = "";
    // let mut code = String::new();
    print!("Filename to load: ");
    stdout().flush().unwrap();
    let mut fname = String::new();
    stdin().read_line(&mut fname).unwrap();
    let code = std::fs::read_to_string(fname.trim()).unwrap();
    // let code = ",.";
    // let code = "+[-->-[>>+>-----<<]<--<---]>-.>>>+.>>..+++[.>]<<<<.+++.------.<<-.>>>>+.";
    // let code = "
    // +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++.
    // ";
    // let code = "
    // +++++[>+++++[>+++++<-]<-]>>
    // ";
    // let code = "--<-<<+[+[<+>--->->->-<<<]>]<<--.<++++++.<<-..<<.<+.>>.>>.<<<.+++.>>.>>-.<<<+.";
    // let code = ",[.,]";
    // let code = "+++++";
    // let code = "+>+>,-[-[->+<]<<[->>>>+>+<<<<<]>[->>>>>+>+<<<<<<]>>>[-<<<<+>>>>]>[-<<<+>>>]>[-<<<<<+>>>>>]>[-<<<<<+>>>>>]<<<<]";
    // let code = "++++[-][-]+++++[-]+++++++";
    let mut parser = Parser::from_str(&code);
    let insns = parser.parse();
    println!("Parsing done");
    print!("Walking length of unoptimized code...");
    stdout().flush().unwrap();
    let l = BFInstruction::walk_len(&insns);
    println!(" {} instructions", l);
    let mut i = 0;
    let opt = OptimizedBFInstruction::optimize(insns, &mut i, l, true);
    println!();
    println!("Optimizing done");
    print!("Walking length of optimized code...");
    stdout().flush().unwrap();
    let l = OptimizedBFInstruction::walk_len(&opt);
    println!(" {} instructions", l);
    // yes();
    // println!("{:?}", insns);
    println!("Running JIT...");
    let mut jit = JIT::new();
    let ptr = jit.jit(&opt);
    println!("JIT'ed into {:x?}", ptr);
    // println!("Attach to me!")
    // stdin().read_line(&mut String::new()).unwrap();
    let mut data = vec![0u64; 134217728]; // 1mib = 1048576, 128mib * 8 = 1gib
    let f = unsafe { std::mem::transmute::<_, BFJitFunction>(ptr) };
    println!("All engines go!");
    let res = f(unsafe { data.as_mut_ptr().add(9000000) });
    println!("Wew done running, got {}", res);
    // println!("Data: {:?}", data);
}
