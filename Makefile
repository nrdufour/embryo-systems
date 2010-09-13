all:  deps compile
	@./rebar get-deps

compile:
	@./rebar compile

clean:
	@./rebar clean

dist: compile
	@rm -rf rel/embryosys
	@./rebar generate

distclean: clean
	@rm -rf rel/embryosys

