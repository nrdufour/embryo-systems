/*
 * Embryo Systems
 *
 * $Id$
 *
 * Copyright 2009 Nicolas R. Dufour <nrdufour@gmail.com>
 * 
 * This source code is released under the BSD License.
 */
package info.embryosys.adt.manager;

/**
 * @author nrdufour
 *
 */
public interface Session {

	void execute(String query);
	
	void doThis(String query);
	
	void commit();
}
