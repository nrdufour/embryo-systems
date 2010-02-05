#!/usr/bin/env escript
%% -*- erlang -*-
%%! -pa ./ebin

main(_) ->
	etap:plan(unknown),
	etap_can:loaded_ok(embryosys_adtm_class, "Module 'embryosys_adtm_class' loaded."),
	etap_can:has_attrib(embryosys_adtm_class, behavior),
	etap_can:is_attrib(embryosys_adtm_class, behavior, gen_server),
	etap_can:can_ok(embryosys_adtm_class, create, 1),
	etap_can:can_ok(embryosys_adtm_class, hibern, 1),
	etap_can:can_ok(embryosys_adtm_class, awake, 1),
	etap_can:can_ok(embryosys_adtm_class, destroy, 1),
	etap_can:can_ok(embryosys_adtm_class, resur, 1),
	etap_can:can_ok(embryosys_adtm_class, purge, 1),

	application:start(embryosys),
	embryosys_storage_server:init_storage(),
	etap:is(embryosys_adtm_class:create("Bridge"), {ok, {class, "Bridge", alive}}, "Creating Class Bridge"),
	etap:is(embryosys_adtm_class:create("Bridge"), {wrong_state, []}, "Try to create Class Bridge again"),
	etap:is(embryosys_adtm_class:create("River"), {ok, {class, "River", alive}}, "Creating Class River"),
	etap:is(embryosys_adtm_class:create("River"), {wrong_state, []}, "Try to create Class River again"),

	etap:is(embryosys_adtm_class:hibern("River"), {ok, {class, "River", frozen}}, "Hibernate Class River"),
	etap:is(embryosys_adtm_class:hibern("River"), {wrong_state, []}, "Try to hibernate again Class River"),
	etap:is(embryosys_adtm_class:hibern("Car"), {wrong_state, []}, "Try to hibernate a class which doesn't exist"),
	application:stop(embryosys),

	etap:end_tests(),
	ok.

