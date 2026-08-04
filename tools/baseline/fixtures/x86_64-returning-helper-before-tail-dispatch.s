0000000000001000 <telomere::runtime::vm::numeric::op_i32_and::h1111111111111111>:
    1000:	48 8b 07	mov    (%rdi),%rax
    1003:	ff e0	jmpq   *%rax
0000000000001010 <telomere::runtime::vm::superinstructions::op_local_get4_br_if::h2222222222222222>:
    1010:	85 c0	test   %eax,%eax
    1012:	74 05	je     1019 <taken>
    1014:	48 8b 07	mov    (%rdi),%rax
    1017:	ff e0	jmpq   *%rax
    1019:	48 8b 07	mov    (%rdi),%rax
    101c:	ff e0	jmpq   *%rax
0000000000001030 <telomere::runtime::vm::memory::op_i32_load_const_base::h3333333333333333>:
    1030:	48 8b 07	mov    (%rdi),%rax
    1033:	ff e0	jmpq   *%rax
0000000000001040 <telomere::runtime::vm::call::op_call::h4444444444444444>:
    1040:	48 8b 07	mov    (%rdi),%rax
    1043:	ff d0	callq  *%rax
    1045:	48 85 c0	test   %rax,%rax
    1048:	74 06	je     1050 <return>
    104a:	48 8b 17	mov    (%rdi),%rdx
    104d:	ff e2	jmpq   *%rdx
    1050:	c3	retq
